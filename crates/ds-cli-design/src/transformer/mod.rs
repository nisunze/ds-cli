//! `ds design transformer` — the map-independent lifecycle of a project's
//! transformer documents: download local rooms, inventory (inspect and plan),
//! retire, restore.
//!
//! Retirement is reversible and non-destructive: it flips the document's
//! soft-delete tombstone that every consumer already honours and records who,
//! when, and why. Nothing is erased; `restore` brings the transformer back.
//! Deletion stays a separate, destructive, paired-application action. All
//! three commands restore the native user and act only on that user's
//! audience-fenced selected project; there is no `--project`, Desktop
//! descriptor, URL, body, or action override.
//!
//! `status` shares this module's credential path, scope flag and refusals but
//! answers `ds design status`: the project's own transformer status rows, the
//! read every other headless Design answer is built from. `dashboard` folds
//! those same rows once into the project's whole progress story. Those two
//! take a REQUIRED `--project` and never consult the saved selection, because
//! a read whose answer depends on where it was run is not one answer.
//!
//! Contract: ds-brain `docs/contracts/transformer-retirement.md`.

pub mod dashboard;
pub mod inventory;
pub mod restore;
pub mod retire;
pub mod status;

use ds_cli_auth::{
    HeadlessProjectReport, PROJECT_REPORT_MAX_REASON_CHARS, PROJECT_REPORT_MAX_TRANSFORMERS,
    RetirementReceipt, RetirementRequest, TransformerInventory, TransformerInventoryRow,
    TransformerSet,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use serde_json::{Value, json};

pub const TRANSFORMER_ARG: Arg = Arg::repeated(
    "transformer",
    "<name>",
    "Exact transformer in the selected headless project. Repeat for several.",
);
pub const REASON_ARG: Arg = Arg::value(
    "reason",
    "<text>",
    "Why the transformer leaves the project's active set; recorded and audited (1-512 characters).",
)
.required();
pub const LANE_ARG: Arg = Arg::value(
    "lane",
    "<stable|canary>",
    "Deployment lane; stable is the default.",
)
.default("stable")
.choices(&["stable", "canary"]);
/// The project a read is about, named rather than inherited.
///
/// A read that silently follows this machine's saved selection answers a
/// different question depending on where it is run, which is exactly what an
/// agent asking the same thing twice cannot tolerate. Naming it is one word
/// more and one ambiguity less.
pub const PROJECT_ARG: Arg = Arg::value(
    "project",
    "<project-id>",
    "Exact project this read is about; the saved selection is not consulted.",
)
.required();

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
// The one signed-out sentence, spelled with its literal code so the source
// scan behind `refusal_coverage.rs` can follow `HEADLESS_SIGNED_OUT.code`.
pub const HEADLESS_SIGNED_OUT: Refusal = Refusal {
    code: "headless_signed_out",
    when: ds_cli_auth::SIGNED_OUT_REFUSAL.when,
    remedy: ds_cli_auth::SIGNED_OUT_REMEDY,
};
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
    "a transformer name or the selected project violates the fixed request bound",
    "pass exact bounded transformer names and select a freshly visible project"
);
refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the fixed gateway rejects the verified request, or the project is archived or expired",
    "verify the account, its project access, and the project lifecycle state"
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
    "the governed report service is temporarily unavailable",
    "retry without changing local state"
);
refusal!(
    AUTH_UNREADABLE,
    "auth_response_unreadable",
    "the response violates its closed bounded contract",
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
    "an explicitly named transformer is blank, untrimmed, or over 200 characters, or more than 500 were named",
    "omit --transformer where the command permits the whole active project, or pass it once per bounded exact name; canonical aliases are de-duplicated"
);
refusal!(
    INVALID_REASON,
    "invalid_reason",
    "--reason is blank, untrimmed, or over 512 characters",
    "pass one bounded reason for the audit record"
);
refusal!(
    CONFIRMATION_REQUIRED,
    "confirmation_required",
    "--yes was not given for a command that changes the project's active transformer set",
    "inspect with `ds design transformer inventory` first, then re-run with --yes"
);

refusal!(
    PROJECT_REQUIRED,
    "project_required",
    "--project is absent, blank or untrimmed",
    "pass one exact ds_project value from ds auth project list"
);
refusal!(
    CONTEXT_CORRUPT,
    "context_corrupt",
    "--project is not one path segment: separator, traversal or whitespace",
    "copy one exact ds_project value from ds auth project list"
);

pub const NATIVE_READ_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
    PROJECT_REQUIRED,
    CONTEXT_CORRUPT,
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
    PROJECT_REQUIRED,
    CONTEXT_CORRUPT,
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
    INVALID_REASON,
    CONFIRMATION_REQUIRED,
];

/// Validate the repeated `--transformer` flag locally so a malformed scope is
/// refused with a remedy before any credential is restored. `require_some`
/// is set for writes; the inventory accepts an empty set (whole project).
pub fn transformer_set(
    inputs: &ds_cli_contract::Inputs,
    require_some: bool,
) -> Result<TransformerSet, Failure> {
    let names = inputs.repeated("transformer");
    if require_some && names.is_empty() {
        return Err(
            Failure::invalid(INVALID_SCOPE.code, "name at least one transformer")
                .remedy(INVALID_SCOPE.remedy),
        );
    }
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

/// The project a named read is about, refused locally before any credential.
///
/// The parser already requires the flag; what it cannot know is that a blank
/// or padded value is not a project id. Refusing it here, by name, means an
/// agent that passed `--project ""` reads `project_required` rather than
/// watching a credential restore fail for no stated reason.
pub fn named_project(inputs: &ds_cli_contract::Inputs) -> Result<String, Failure> {
    let project = inputs.require("project")?;
    if project.trim().is_empty() || project.trim() != project {
        return Err(Failure::invalid(
            PROJECT_REQUIRED.code,
            "--project must be one exact, trimmed, non-empty project id",
        )
        .remedy(PROJECT_REQUIRED.remedy)
        .next("ds auth project list --output json"));
    }
    Ok(project.to_owned())
}

/// The receipt of a read whose project the caller named.
///
/// It carries the id it was given and nothing it did not read: no
/// `project_name`, no `status`. Those live in the saved selection, and this
/// read never opened it.
pub fn named_project_receipt(lane: &str, ds_project: &str) -> Value {
    json!({ "lane": lane, "project": { "ds_project": ds_project } })
}

pub fn reason(inputs: &ds_cli_contract::Inputs) -> Result<String, Failure> {
    let reason = inputs.require("reason")?;
    if reason.trim() != reason
        || reason.is_empty()
        || reason.chars().count() > PROJECT_REPORT_MAX_REASON_CHARS
    {
        return Err(Failure::invalid(
            "invalid_reason",
            "the reason must be trimmed, non-empty, and at most 512 characters",
        )
        .remedy(INVALID_REASON.remedy));
    }
    Ok(reason.to_owned())
}

/// The lane and the project a headless read answered for. The project is the
/// one the caller named, so its display name and status are only present when
/// the owner supplied them; nothing is invented from a saved selection.
pub fn project_receipt<T>(headless: &HeadlessProjectReport<T>) -> Value {
    let mut project = json!({ "ds_project": headless.project_id() });
    if !headless.project_name().is_empty() {
        project["project_name"] = json!(headless.project_name());
    }
    if !headless.project_status().is_empty() {
        project["status"] = json!(headless.project_status());
    }
    json!({ "lane": headless.lane(), "project": project })
}

/// The verified backup ds-brain wrote before a retirement: the exact object a
/// deleted transformer is restored from, with the digest that proves it.
fn backup_json(backup: &ds_cli_auth::RetirementBackup) -> Value {
    json!({
        "bucket": backup.bucket(),
        "object": backup.object(),
        "generation": backup.generation(),
        "sha256": backup.sha256(),
        "byte_length": backup.byte_length(),
    })
}

pub fn row_json(row: &TransformerInventoryRow) -> Value {
    let mut out = json!({
        "name": row.name(),
        "kind": row.kind().token(),
        "state": row.lifecycle().token(),
    });
    if let Some(record) = row.retirement() {
        out["retirement"] = json!({
            "reason": record.reason(),
            "retired_at": record.retired_at(),
            "retired_by": record.retired_by(),
            "restored_at": record.restored_at(),
            "restored_by": record.restored_by(),
        });
        if let Some(backup) = record.backup() {
            out["retirement"]["backup"] = backup_json(backup);
        }
    }
    out
}

pub fn inventory_json(inventory: &TransformerInventory) -> Value {
    json!({
        "requested": inventory.requested(),
        "active_count": inventory.active_count(),
        "retired_count": inventory.retired_count(),
        "deleted_count": inventory.deleted_count(),
        "transformers": inventory.rows().iter().map(row_json).collect::<Vec<_>>(),
    })
}

pub fn receipt_json(receipt: &RetirementReceipt, request: &RetirementRequest) -> Value {
    json!({
        "action": receipt.action().token(),
        "requested": request.transformers().names(),
        "reason": request.reason(),
        "applied_count": receipt.applied_count(),
        "failed_count": receipt.failed_count(),
        "results": receipt.results().iter().map(|result| {
            let mut out = json!({
                "name": result.name(),
                "applied": result.applied(),
            });
            if let Some(at) = result.at() {
                out["at"] = json!(at);
            }
            if let Some(refusal) = result.refusal() {
                out["refusal"] = json!(refusal.token());
            }
            if let Some(error) = result.error() {
                out["error"] = json!(error);
            }
            if let Some(backup) = result.backup() {
                out["backup"] = backup_json(backup);
            }
            out
        }).collect::<Vec<_>>(),
    })
}

/// `name (id)` when the receipt carries a name, else the id the caller named.
pub fn project_label(data: &Value) -> String {
    let id = data["project"]["ds_project"].as_str().unwrap_or("?");
    match data["project"]["project_name"].as_str() {
        Some(name) if !name.is_empty() => format!("{name} ({id})"),
        _ => id.to_owned(),
    }
}

pub fn render_receipt(verb: &str, data: &Value) -> String {
    let mut out = format!(
        "project {} · {} · {verb}: {} applied, {} failed\n",
        project_label(data),
        data["lane"].as_str().unwrap_or("?"),
        data["applied_count"].as_u64().unwrap_or(0),
        data["failed_count"].as_u64().unwrap_or(0),
    );
    if let Some(results) = data["results"].as_array() {
        for result in results {
            let name = result["name"].as_str().unwrap_or("?");
            if result["applied"].as_bool().unwrap_or(false) {
                match result["backup"]["object"].as_str() {
                    Some(object) => out.push_str(&format!(
                        "  {name:<32} {verb} · backup gs://{}/{object}#{} sha256:{}\n",
                        result["backup"]["bucket"].as_str().unwrap_or("?"),
                        result["backup"]["generation"],
                        result["backup"]["sha256"].as_str().unwrap_or("?"),
                    )),
                    None => out.push_str(&format!("  {name:<32} {verb}\n")),
                }
            } else {
                out.push_str(&format!(
                    "  {name:<32} refused ({}): {}\n",
                    result["refusal"].as_str().unwrap_or("failed"),
                    result["error"].as_str().unwrap_or(""),
                ));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    #[test]
    fn an_applied_retirement_shows_the_backup_it_can_be_restored_from() {
        let data = json!({
            "lane": "canary",
            "project": {"ds_project": "p1"},
            "applied_count": 1,
            "failed_count": 0,
            "results": [{
                "name": "TX-1",
                "applied": true,
                "backup": {
                    "bucket": "ds-transformer-backups",
                    "object": "p1/TX-1/1.json",
                    "generation": 7,
                    "sha256": "ab",
                    "byte_length": 10,
                },
            }],
        });
        let text = super::render_receipt("retired", &data);
        assert!(
            text.contains("backup gs://ds-transformer-backups/p1/TX-1/1.json#7 sha256:ab"),
            "{text}"
        );
    }
}
