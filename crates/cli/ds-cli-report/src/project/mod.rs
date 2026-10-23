//! `ds report project` — report commands over one explicitly named project.
//!
//! Every computation runs on this edge. `export` and `combined` compute the
//! individual and Combined Reports through the installed Reporter;
//! `compounded` composes the Compounded Report ZIP from the current report
//! bytes this machine retains, through the Reporter's closed bundle task, and
//! publishes it through the governed compute-artifact door. ds-brain only
//! authorizes, stores and lists: it never computes, composes or regenerates a
//! report. The CLI never holds a token.
//!
//! ```text
//!   scope → export / combined → compounded → archives
//! ```
//!
//! `scope` is the plan: it reads the lifecycle inventory and shows which
//! transformers participate and which are excluded (retired, deleted, missing)
//! before any artifact is produced.

pub mod archives;
pub mod combined;
pub mod combined_export;
pub mod crs;
pub mod export;
pub mod grouping;
pub mod hold;
pub mod map_inputs;
pub mod mv_setup;
mod neighbor_points;
pub mod publish;
pub mod reuse;
pub mod scope;
pub mod settings;
mod survey_context;
mod vd_preview;

use ds_cli_auth::{
    HeadlessNamedProject, HeadlessProjectReport, PROJECT_REPORT_MAX_TRANSFORMERS,
    TransformerInventory, TransformerSet,
};
use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use serde_json::{Value, json};

pub const TRANSFORMER_ARG: Arg = Arg::repeated(
    "transformer",
    "<name>",
    "Explicit scope; repeat per name. Omit for every active saved transformer.",
);
pub const LANE_ARG: ds_cli_contract::spec::Arg =
    ds_cli_contract::spec::LANE.summary("Deployment lane; stable is the default.");
pub const PROJECT_ARG: Arg =
    Arg::value("project", "<ds-project>", "Project named for this request.").required();

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
pub const HEADLESS_SIGNED_OUT: Refusal = ds_cli_auth::SIGNED_OUT_REFUSAL;
pub const RECONCILIATION_REQUIRED: Refusal = Refusal {
    code: "report_reconciliation_required",
    when: "the named project's report sync pass could not finish before a publication or mutation",
    remedy: "restore this machine's report sync connection, then retry the same command",
};
// Legacy planning commands still use saved context until their domain migrates.
pub const HEADLESS_NO_PROJECT: Refusal = Refusal {
    code: "headless_project_not_selected",
    when: "the user has no audience-fenced selected project",
    remedy: "run ds auth project use --project <exact-id>",
};
pub const PROJECT_CONTEXT_STALE: Refusal = Refusal {
    code: "project_context_stale",
    when: "the saved project belongs to another identity, lane, or audience",
    remedy: "select the project again with ds auth project use",
};
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
    "native providers disagree on identity or project",
    "sign out or revoke the unintended provider"
);
refusal!(
    AUTH_INPUT,
    "auth_input_invalid",
    "an excluded transformer, no `report_archive` grouping, or bad scope",
    "ds report project scope; ds design consumer-grouping read --purpose report_archive"
);
refusal!(
    AUTH_REJECTED,
    "auth_rejected",
    "the gateway rejects the request, design.compounded_report is missing, or the project is archived",
    "verify the account, project access and capabilities"
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
    "the report service is temporarily unavailable or freshness is unverified",
    "retry; local state is unchanged"
);
refusal!(
    AUTH_UNREADABLE,
    "auth_response_unreadable",
    "the response violates its closed contract",
    "retry once, then update ds if it persists"
);
refusal!(
    NOT_FOUND,
    "transformer_not_found",
    "the service found no such project",
    "check the exact --project ID and this account's access"
);
refusal!(
    INVALID_SCOPE,
    "invalid_transformer_scope",
    "a --transformer name is blank, untrimmed, over 200 characters, or over 500 names",
    "pass bounded names, or omit them for every active transformer"
);
refusal!(
    RESERVED_IDENTITY,
    "reserved_transformer_identity",
    "a --transformer name is a reserved computed identity (collisions, combined_transformer)",
    "drop it; `ds report project scope` lists participants"
);
refusal!(
    CONFIRMATION_REQUIRED,
    "confirmation_required",
    "--yes was not given for a command that publishes a durable archive",
    "run `ds report project scope` first, then re-run with --yes"
);

pub const NATIVE_READ_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
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
    RESERVED_IDENTITY,
    REPORT_NO_INDIVIDUAL_ARTIFACTS,
    REPORT_GROUPING_STALE,
    REPORT_GROUPING_INCOMPLETE,
    COMBINED_INPUTS_PUBLICATION_PENDING,
    COMBINED_INPUTS_NOT_CURRENT,
    COMBINED_INPUTS_EMPTY,
    COMBINED_NO_INPUTS,
    COMBINED_READINESS_UNAVAILABLE,
];

/// The governed report route names these itself. They were previously
/// indistinguishable — every one arrived as `auth_input_invalid` with no
/// detail and no remedy, because the client classified on the HTTP status
/// alone and `409` covers two of them.
const REPORT_NO_INDIVIDUAL_ARTIFACTS: Refusal = Refusal {
    code: "report_no_individual_artifacts",
    when: "no selected transformer had an individual report to package",
    remedy: "generate the individual reports first, then retry",
};
const REPORT_GROUPING_STALE: Refusal = Refusal {
    code: "report_grouping_stale",
    when: "the applied grouping changed after this request pinned its digest",
    remedy: "re-read the applied grouping and retry with its new digest",
};
const REPORT_GROUPING_INCOMPLETE: Refusal = Refusal {
    code: "report_grouping_incomplete",
    when: "the applied grouping does not cover every requested transformer",
    remedy: "apply a grouping that covers them, or narrow the requested scope",
};

/// The four ways a Compounded Report refuses over its input rooms, and the one
/// way the predicate itself cannot answer.
///
/// They are declared here rather than built from the kernel's reply so that
/// `ds capabilities report.project.compounded` lists every code the command can
/// emit, and so `refusal_coverage` can see them. The kernel owns WHICH one
/// applies; this owns the fact that it exists.
pub const COMBINED_INPUTS_PUBLICATION_PENDING: Refusal = Refusal {
    code: "combined_inputs_publication_pending",
    when: "a room's report has not left its device publication queue",
    remedy: "run `ds report outbox drain` on that device",
};
pub const COMBINED_INPUTS_NOT_CURRENT: Refusal = Refusal {
    code: "combined_inputs_not_current",
    when: "a room's report is missing, stale, failed or abandoned by the queue",
    remedy: "regenerate those rooms with `ds report project export --transformer <name>`",
};
pub const COMBINED_INPUTS_EMPTY: Refusal = Refusal {
    code: "combined_inputs_empty",
    when: "every room in scope is current and holds nothing to combine",
    remedy: "read `ds report project scope`; the design is what is empty",
};
pub const COMBINED_NO_INPUTS: Refusal = Refusal {
    code: "combined_no_inputs",
    when: "no room is in scope at all",
    remedy: "widen the scope, then confirm it with `ds report project scope`",
};
pub const COMBINED_READINESS_UNAVAILABLE: Refusal = Refusal {
    code: "combined_readiness_unavailable",
    when: "the readiness predicate could not answer",
    remedy: "report this build; nothing was published",
};

pub const NATIVE_WRITE_REFUSALS: &[Refusal] = &[
    NATIVE_PROFILE,
    NATIVE_PROFILE_DIGEST,
    NATIVE_PROFILE_UNSAFE,
    HEADLESS_SIGNED_OUT,
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
    RESERVED_IDENTITY,
    CONFIRMATION_REQUIRED,
    RECONCILIATION_REQUIRED,
    REPORT_NO_INDIVIDUAL_ARTIFACTS,
    REPORT_GROUPING_STALE,
    REPORT_GROUPING_INCOMPLETE,
];

/// Several declared refusal lists as one, at compile time, so a command's
/// list is still one `&'static [Refusal]` its help and descriptor read.
pub(crate) const fn joined<const N: usize>(parts: &[&[Refusal]]) -> [Refusal; N] {
    let mut out = [NATIVE_PROFILE; N];
    let (mut at, mut part) = (0, 0);
    while part < parts.len() {
        let mut index = 0;
        while index < parts[part].len() {
            out[at] = parts[part][index];
            at += 1;
            index += 1;
        }
        part += 1;
    }
    assert!(at == N, "joined refusal lists must fill the array exactly");
    out
}

const fn same_code(left: &str, right: &str) -> bool {
    let (left, right) = (left.as_bytes(), right.as_bytes());
    if left.len() != right.len() {
        return false;
    }
    let mut index = 0;
    while index < left.len() {
        if left[index] != right[index] {
            return false;
        }
        index += 1;
    }
    true
}

const fn first_of_code(parts: &[&[Refusal]], part: usize, index: usize) -> bool {
    let code = parts[part][index].code;
    let mut p = 0;
    while p <= part {
        let end = if p == part { index } else { parts[p].len() };
        let mut i = 0;
        while i < end {
            if same_code(parts[p][i].code, code) {
                return false;
            }
            i += 1;
        }
        p += 1;
    }
    true
}

/// How many distinct codes several declared refusal lists hold.
pub const fn distinct_count(parts: &[&[Refusal]]) -> usize {
    let (mut count, mut part) = (0, 0);
    while part < parts.len() {
        let mut index = 0;
        while index < parts[part].len() {
            if first_of_code(parts, part, index) {
                count += 1;
            }
            index += 1;
        }
        part += 1;
    }
    count
}

/// Several declared refusal lists as one, each code once (first wins), so a
/// command composed of other commands' owners declares every code it can emit.
pub const fn distinct<const N: usize>(parts: &[&[Refusal]]) -> [Refusal; N] {
    let mut out = [NATIVE_PROFILE; N];
    let (mut at, mut part) = (0, 0);
    while part < parts.len() {
        let mut index = 0;
        while index < parts[part].len() {
            if first_of_code(parts, part, index) {
                out[at] = parts[part][index];
                at += 1;
            }
            index += 1;
        }
        part += 1;
    }
    assert!(
        at == N,
        "distinct refusal lists must fill the array exactly"
    );
    out
}

/// `scope`: the read lane plus the grouping's own refusals.
pub const SCOPE_REFUSALS: &[Refusal] = &joined::<
    { NATIVE_READ_REFUSALS.len() + grouping::REFUSALS.len() },
>(&[NATIVE_READ_REFUSALS, &grouping::REFUSALS]);

/// `compounded`: the write lane, the readiness refusals its inputs are read
/// through, the composition's and grouping's own refusals, the grouped run
/// that did not publish every group, everything the combined set computed
/// here can refuse, and the bundle and seal steps.
const COMPOUNDED_PARTS: &[&[Refusal]] = &[
    NATIVE_WRITE_REFUSALS,
    &[
        COMBINED_INPUTS_PUBLICATION_PENDING,
        COMBINED_INPUTS_NOT_CURRENT,
        COMBINED_INPUTS_EMPTY,
        COMBINED_NO_INPUTS,
        COMBINED_READINESS_UNAVAILABLE,
        Refusal {
            code: "compounded_composition_invalid",
            when: "composition JSON is malformed, oversized or outside ds.compounded-report/v1",
            remedy: "correct the JSON: schema, transformers, archive_layout {group_depth, transformer_folders, combine_per_group} and force",
        },
        Refusal {
            code: "compounded_composition_conflict",
            when: "composition is mixed with scope, grouping, layout or force flags",
            remedy: "pass composition or ordinary flags",
        },
        combined::GROUPING_INVALID,
        combined::INPUTS_INVALID,
        combined::ARCHIVE_TOO_LARGE,
        combined::STORE_UNREADABLE,
        crate::ENGINE_RECEIPT_INVALID,
        crate::ENGINE_TASK_UNKNOWN,
    ],
    &grouping::REFUSALS,
    &[grouping::GROUPS_PARTIAL],
    combined_export::REFUSALS,
];

pub const COMBINED_REFUSALS: &[Refusal] =
    &distinct::<{ distinct_count(COMPOUNDED_PARTS) }>(COMPOUNDED_PARTS);

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
    let requested = TransformerSet::new(names.iter().cloned()).map_err(|error| {
        Failure::invalid("invalid_transformer_scope", error.to_string())
            .remedy(INVALID_SCOPE.remedy)
    })?;
    // The reserved identities are what a combined run produces, not who it
    // runs over. The offline lane already refuses them, so the same kernel
    // predicate answers here and the plan cannot admit a participant the
    // publish would have to invent. Both the name as given and its canonical
    // form are checked: canonicalization folds several spellings onto one key.
    let mut reserved: Vec<String> = names
        .iter()
        .chain(requested.names())
        .filter(|name| ds_command_kernel::report::is_reserved_report_identity(name))
        .cloned()
        .collect();
    reserved.sort();
    reserved.dedup();
    if !reserved.is_empty() {
        return Err(Failure::invalid(
            "reserved_transformer_identity",
            format!(
                "reserved computed identity in scope: {}",
                reserved.join(", ")
            ),
        )
        .remedy(RESERVED_IDENTITY.remedy));
    }
    Ok(requested)
}

pub trait ProjectReceipt {
    fn lane(&self) -> &'static str;
    fn project_id(&self) -> &str;
    fn project_name(&self) -> Option<&str>;
    fn project_status(&self) -> Option<&str>;
}

impl<T> ProjectReceipt for HeadlessProjectReport<T> {
    fn lane(&self) -> &'static str {
        self.lane()
    }
    fn project_id(&self) -> &str {
        self.project_id()
    }
    fn project_name(&self) -> Option<&str> {
        Some(self.project_name())
    }
    fn project_status(&self) -> Option<&str> {
        Some(self.project_status())
    }
}

impl<T> ProjectReceipt for HeadlessNamedProject<T> {
    fn lane(&self) -> &'static str {
        self.lane()
    }
    fn project_id(&self) -> &str {
        self.project_id()
    }
    fn project_name(&self) -> Option<&str> {
        None
    }
    fn project_status(&self) -> Option<&str> {
        None
    }
}

pub fn project_receipt(headless: &impl ProjectReceipt) -> Value {
    json!({
        "lane": headless.lane(),
        "project": {
            "ds_project": headless.project_id(),
            "project_name": headless.project_name(),
            "status": headless.project_status(),
        },
    })
}

/// The exact scope a combined run would use, derived from the lifecycle
/// inventory: active ordinary transformers participate; retired, deleted and
/// missing names are excluded with their state. Project-level documents are
/// shown for inventory context; they never participate in the LV report.
pub fn scope_json(requested: &TransformerSet, inventory: &TransformerInventory) -> Value {
    scope_rows_json(requested, &hold::rows(inventory))
}

/// [`scope_json`] over inventory rows, whether the service just answered them
/// or this machine holds them.
pub fn scope_rows_json(requested: &TransformerSet, rows: &[hold::Row]) -> Value {
    let mut participating = Vec::new();
    let mut excluded = Vec::new();
    let mut project_level = Vec::new();
    for row in rows {
        if !row.is_transformer() {
            project_level.push(json!({"name": row.name, "state": row.state}));
            continue;
        }
        if row.is_active() {
            participating.push(row.name.clone());
        } else {
            let mut entry = json!({"name": row.name, "state": row.state});
            if row.retired {
                entry["reason"] = json!(row.reason);
            }
            excluded.push(entry);
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
        // `compounded_ready` describes the archive scope. `combined_ready` is
        // an existing compatibility key for readers of the old CLI contract.
        "combined_ready": !participating.is_empty(),
        "compounded_ready": !participating.is_empty(),
    })
}

/// How a Compounded Report archive is laid out, in the report layer's own vocabulary.
///
/// Two spellings describe one choice — `file_level` is current and
/// `transformer_grouping` is the legacy twin older archives were written with —
/// and folding them was, until now, a nested ternary inside one Svelte
/// component. `ds` therefore produced archives it could not describe, and
/// described legacy archives as having no layout at all. The fold is
/// `ds_command_kernel::report::ArchiveLayout`'s; this hands it the two
/// spellings and returns the keys a caller renders.
pub fn archive_layout_vocabulary(
    file_level: Option<&str>,
    transformer_grouping: Option<&str>,
    combine_per_district: bool,
) -> serde_json::Value {
    ds_command_kernel::report::ArchiveLayout {
        file_level: file_level.map(str::to_string),
        transformer_grouping: transformer_grouping.map(str::to_string),
        combine_per_district: Some(combine_per_district),
        // This helper only supplies legacy vocabulary; the owner defaults any
        // other choices instead of the CLI copying optional schema fields.
        ..Default::default()
    }
    .describe()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_project_report_command_names_its_project() {
        for command in [
            &scope::COMMAND,
            &crs::COMMAND,
            &combined::COMMAND,
            &combined_export::COMMAND,
            &archives::COMMAND,
            &settings::COMMAND,
            &settings::OUTPUTS_SET,
            &export::COMMAND,
            &publish::COMMAND,
            &map_inputs::COMMAND,
        ] {
            let project = command
                .arg("project")
                .expect("report command lacks --project");
            assert!(project.required, "{} defaults its project", command.id);
            assert!(
                project.default.is_none(),
                "{} has a project default",
                command.id
            );
        }
    }

    /// The refusal every `report project` help screen carries has to name
    /// causes an operator can reach through `ds`. A blocked collision policy
    /// is not one: no command here models a collision — it is a GUI Analysis
    /// concept — and `ds report project scope` ran green before every failure
    /// of the stress session, so it cannot be the whole remedy either.
    #[test]
    fn auth_input_names_reachable_causes_and_both_doors() {
        assert!(
            !AUTH_INPUT.when.contains("collision"),
            "phantom cause is back: {}",
            AUTH_INPUT.when
        );
        assert!(
            AUTH_INPUT.when.contains("transformer"),
            "{}",
            AUTH_INPUT.when
        );
        assert!(
            AUTH_INPUT.when.contains("`report_archive` grouping"),
            "{}",
            AUTH_INPUT.when
        );
        assert!(
            AUTH_INPUT.remedy.contains("ds report project scope"),
            "{}",
            AUTH_INPUT.remedy
        );
        assert!(
            AUTH_INPUT
                .remedy
                .contains("ds design consumer-grouping read --purpose report_archive"),
            "the grouping door is missing: {}",
            AUTH_INPUT.remedy
        );
    }

    /// Both lanes carry it, so `scope`, `combined` and `archives` all state
    /// the same prerequisite.
    #[test]
    fn auth_input_reaches_every_report_project_help_screen() {
        for refusals in [NATIVE_READ_REFUSALS, NATIVE_WRITE_REFUSALS] {
            assert!(
                refusals
                    .iter()
                    .any(|refusal| refusal.code == AUTH_INPUT.code),
                "auth_input_invalid left a report project lane"
            );
        }
    }
}

pub(crate) mod mv_context;
