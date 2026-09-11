//! `ds design selection` — a named Transformer Status selection.
//!
//! ```text
//!   list → read → save | archive | assign
//! ```
//!
//! `read` is the load-bearing step. It EVALUATES membership server-side and
//! reports each member as `present`, `changed` or `missing` — a member whose
//! transformer no longer exists is named under the label it was saved with,
//! never substituted. It also returns the member digest, which `assign` echoes
//! back: that echo is what proves the operator saw the exact set being assigned,
//! and a promotion whose membership moved in between is refused rather than
//! quietly assigning a different set of work.
//!
//! **These five commands are NATIVE, and the decision did not move.** Membership
//! is ds-brain's answer and nothing here re-derives it; what was wrong was
//! residency — a server-owned answer needed a signed-in browser to read, purely
//! because the credential lived there. `ds-client-core::design_selections` is
//! that answer's native residency, `ds_cli_auth::design_selections` restores the
//! same audience-fenced project every other headless Design read uses, and the
//! answers below are byte-for-byte the ones the bridge returned.
//!
//! The lasso stays paired. A selection drawn on the map is `map.selection.*`
//! under the paired application, because it is made by pointing at a rendered
//! map — there is nothing headless about it. A SAVED selection is a document.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Arg, Refusal};
use ds_client_core::{
    DesignSelectionAnswer, DesignSelectionRead, DesignSelectionRequest, DesignSelectionSummary,
};
use serde_json::{Value, json};

pub mod archive;
pub mod assign;
pub mod list;
pub mod read;
pub mod save;

pub const LANE: Arg = Arg::value("lane", "<stable|canary>", "Deployment lane.")
    .default("stable")
    .choices(&["stable", "canary"]);

/// A selection id a caller may pin instead of letting `ds` mint one.
pub const ID_ARG: Arg = Arg::value(
    "id",
    "<selection-id>",
    "The id to create under. Omit and one is minted from the name.",
);

const NOT_FOUND: Refusal = Refusal {
    code: "design_selection_not_found",
    when: "No saved selection in the selected project has that id",
    remedy: "List the project's selections with `ds design selection list`",
};

const REFUSED: Refusal = Refusal {
    code: "auth_rejected",
    when: "This account may not read or change saved selections in this project",
    remedy: "Ask a project admin for the design selection capability",
};

const MOVED: Refusal = Refusal {
    code: "auth_input_invalid",
    when: "The selection moved between the read and the write, or an input is outside ds-brain's grammar",
    remedy: "Read the selection again with `ds design selection read` and repeat the change",
};

/// Every refusal these commands can answer with: the native spine's, plus the
/// three this surface adds.
pub const REFUSALS: &[Refusal] = &[
    ds_cli_report::project::NATIVE_PROFILE,
    ds_cli_report::project::NATIVE_PROFILE_DIGEST,
    ds_cli_report::project::NATIVE_PROFILE_UNSAFE,
    ds_cli_report::project::HEADLESS_SIGNED_OUT,
    ds_cli_report::project::HEADLESS_NO_PROJECT,
    ds_cli_report::project::PROJECT_CONTEXT_STALE,
    ds_cli_report::project::NATIVE_STATE_UNSAFE,
    ds_cli_report::project::NATIVE_STATE_UNAVAILABLE,
    NOT_FOUND,
    REFUSED,
    MOVED,
];

/// One saved-selection call against the selected project.
///
/// Returns the project id with the answer because every one of these commands
/// reports it, and because it is the only project a headless `ds` may reach.
pub fn ask(
    lane: &str,
    selection: &str,
    request: &DesignSelectionRequest,
) -> Result<(String, DesignSelectionAnswer), Failure> {
    let report = ds_cli_auth::design_selections(lane, request).map_err(|failure| {
        // The shared kind mapping speaks of transformers, because until now a
        // 404 on a project route always was one. A missing selection has its
        // own name and its own way out.
        if failure.code() == "transformer_not_found" {
            return Failure::invalid(
                NOT_FOUND.code,
                format!("no saved selection {selection} in the selected project"),
            )
            .remedy(NOT_FOUND.remedy)
            .next("ds design selection list --output json");
        }
        failure
    })?;
    let project = report.project_id().to_owned();
    Ok((project, report.into_result()))
}

/// Read one selection's evaluated membership. Every write starts here: the
/// version and the member digest are READ, never asserted by the caller.
pub fn read_selection(
    lane: &str,
    selection: &str,
) -> Result<(String, DesignSelectionRead), Failure> {
    let (project, answer) = ask(
        lane,
        selection,
        &DesignSelectionRequest::Read {
            selection_id: selection.to_owned(),
        },
    )?;
    match answer {
        DesignSelectionAnswer::Read(read) => Ok((project, *read)),
        _ => Err(unexpected()),
    }
}

pub fn saved_head(answer: DesignSelectionAnswer) -> Result<DesignSelectionSummary, Failure> {
    match answer {
        DesignSelectionAnswer::Saved(head) => Ok(head),
        _ => Err(unexpected()),
    }
}

fn unexpected() -> Failure {
    Failure::unavailable(
        "auth_response_unreadable",
        "the saved-selection response did not match its closed contract",
    )
}

/// The selection id `ds` creates under when the caller pins none.
///
/// Seeded from the human name like the application's own mint, because a name
/// is what an operator will look for; the suffix is what makes a second "Week
/// 32 review" a second record rather than a blind overwrite of somebody else's.
/// The suffix is the machine clock, not randomness: `ds` mints one id per
/// command, and a clock that has moved is enough to separate them.
pub fn mint_id(prefix: &str, human: &str) -> String {
    let slug: String = human
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let slug = slug.trim_matches('-').replace("--", "-");
    let slug: String = slug.chars().take(96).collect();
    let slug = slug.trim_matches('-').to_owned();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_nanos());
    let suffix = radix36(now);
    let base = if slug.is_empty() {
        prefix.to_owned()
    } else {
        format!("{prefix}-{slug}")
    };
    let minted: String = format!("{base}-{suffix}").chars().take(128).collect();
    minted.trim_end_matches('-').to_owned()
}

fn radix36(mut value: u128) -> String {
    const DIGITS: &[u8] = b"0123456789abcdefghijklmnopqrstuvwxyz";
    if value == 0 {
        return "0".to_owned();
    }
    let mut out = Vec::new();
    while value > 0 {
        out.push(DIGITS[(value % 36) as usize]);
        value /= 36;
    }
    out.reverse();
    String::from_utf8(out).expect("base-36 digits are ASCII")
}

/// One selection head, in the shape the register's own CLI answer has always
/// had. The residency moved; the answer did not.
pub fn head_json(project: &str, head: &DesignSelectionSummary) -> Value {
    json!({
        "project": project,
        "selection": head.selection_id,
        "name": head.name,
        "mode": head.mode,
        "version": head.version,
        "state": head.state,
    })
}
