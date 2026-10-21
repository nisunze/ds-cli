//! One answer to "may this account see this project?" for the calls that would
//! otherwise answer an empty success about a project that does not exist.
//!
//! A named project that the service has never heard of reads as an empty plan,
//! an empty asset index or an empty sync store, and a dry-run of a write
//! against it succeeds. The directory the gateway lists for the signed-in
//! account is the only authority on visibility, so the check is that listing,
//! read once per process.

use std::collections::BTreeSet;
use std::sync::Mutex;

use ds_cli_contract::outcome::Failure;
use serde_json::Value;

/// Projects already proved visible in this process, per lane.
static PROVEN: Mutex<BTreeSet<(String, String)>> = Mutex::new(BTreeSet::new());

/// The refusal for a project the account cannot see; the one code every
/// project-scoped command already declares.
pub fn not_visible() -> Failure {
    Failure::invalid(
        "project_not_visible",
        "that exact project id is not present in the project directory the gateway returned",
    )
    .remedy("choose an exact id from `ds auth project list`")
    .next("ds auth project list")
}

/// Whether one directory row list contains the exact project.
pub(crate) fn listed(rows: &[Value], project: &str) -> bool {
    rows.iter()
        .any(|row| row["ds_project"].as_str() == Some(project))
}

/// A directory that could not be fetched because the network is away proves
/// nothing about visibility, so retained data stays usable offline. Every
/// other failure (signed out, revoked, refused) is the caller's to see.
pub(crate) fn directory_unreachable(failure: &Failure) -> bool {
    failure.code() == "auth_transient"
}

/// Refuse a project that is not in the signed-in account's directory.
///
/// The directory is fetched at most once per process for a given lane and
/// project. When it cannot be reached the check passes: offline reads of
/// retained data are governed by what was retained, and every online effect is
/// still authorized by the service.
pub fn require_project_visible(lane: &str, project: &str) -> Result<(), Failure> {
    let key = (lane.to_owned(), project.to_owned());
    if PROVEN.lock().is_ok_and(|proven| proven.contains(&key)) {
        return Ok(());
    }
    match crate::project_directory(lane) {
        Ok(directory) => {
            if !listed(directory.projects(), project) {
                return Err(not_visible());
            }
        }
        Err(failure) if directory_unreachable(&failure) => return Ok(()),
        Err(failure) => return Err(failure),
    }
    if let Ok(mut proven) = PROVEN.lock() {
        proven.insert(key);
    }
    Ok(())
}

/// Settle a project-scoped answer lazily. The directory is a whole-account
/// listing and costs seconds for an elevated account, so a call that returned
/// real content never pays it. Only an answer that is exactly what a missing
/// project also produces (`empty`), or a refusal a missing project could have
/// caused, is checked against it. A project the account cannot see then
/// refuses by name instead of reading as an empty success or an unrelated
/// error; any other result is returned unchanged.
pub(crate) fn refuse_unseen(
    lane: &str,
    project: &str,
    result: Result<crate::HeadlessNamedProject<Value>, Failure>,
    empty: impl Fn(&Value) -> bool,
) -> Result<crate::HeadlessNamedProject<Value>, Failure> {
    match result {
        Ok(named) => {
            if empty(&named.result) {
                require_project_visible(lane, project)?;
            }
            Ok(named)
        }
        Err(failure) => {
            if could_follow_missing_project(&failure)
                && matches!(require_project_visible(lane, project), Err(ref f) if f.code() == "project_not_visible")
            {
                return Err(not_visible());
            }
            Err(failure)
        }
    }
}

/// A refusal that a missing project can explain. Authorization, conflicts and
/// a network that is away say nothing about whether the project exists.
pub(crate) fn could_follow_missing_project(failure: &Failure) -> bool {
    use ds_cli_contract::ExitClass::{Conflict, Unauthorized};
    !matches!(failure.class(), Unauthorized | Conflict)
        && !directory_unreachable(failure)
        && failure.code() != "project_not_visible"
}
