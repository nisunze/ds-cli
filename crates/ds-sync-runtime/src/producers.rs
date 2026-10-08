//! The one producer set every host runs.
//!
//! The Desktop is the Server's core with a window, so both name the same
//! engines with the same producers: committed Network Reporter batches under
//! the host's report-artifact root, and Solar's prepared jobs and retained
//! native publications in the host's durable database. Each producer moves
//! bytes through the one native transport. A host supplies only its paths and
//! its captured identity; it builds no producer, upload closure or engine list
//! of its own.

use std::path::Path;
use std::sync::Mutex;

use ds_compute_runtime::HostIdentity;

use crate::solar_producer::SolarProducer;
use crate::{reports, solar, Producer, Producers};

/// Build the set for one project's pass and hand it to `f`. `failure` is the
/// Server pump's failure sentence, projected into Solar activity rows; a host
/// with no pump passes `None`.
pub fn with_producers<T>(
    database: &Path,
    report_root: &Path,
    identity: &HostIdentity,
    project: &str,
    failure: Option<&Mutex<Option<String>>>,
    f: impl FnOnce(&Producers<'_>) -> Result<T, String>,
) -> Result<T, String> {
    let upload =
        |handle: ds_report_artifacts::SealedArtifactHandle, output_id: &str, session_uri: &str| {
            crate::transfer_verified_output(
                output_id,
                session_uri,
                handle.size_bytes,
                &handle.sha256,
                handle.file,
            )
        };
    let report = reports::ReportProducer {
        root: report_root,
        project,
        upload: &upload,
    };
    let solar = SolarProducer {
        database,
        identity,
        project: project.to_owned(),
        failure,
    };
    let producers = Producers::new(vec![
        (reports::ENGINE, &report as &dyn Producer),
        (solar::ENGINE, &solar as &dyn Producer),
    ])?;
    f(&producers)
}

/// The projects a host holds publications for, read from the work itself and
/// never from a selection: report batches under its report root and every
/// row the store holds under its fence — Solar's sealed completions and
/// retained publications among them. A host restarts and wakes its
/// publication passes from this one inventory. `report_owner` narrows report
/// batches on a root several accounts share (a Desktop installation); a
/// Server, whose root belongs to its one owner, passes `None`. An absent
/// store contributes nothing and is not created.
pub fn publication_projects(
    database: &Path,
    report_root: &Path,
    fence: &crate::store::Fence,
    report_owner: Option<&str>,
) -> Result<std::collections::BTreeSet<String>, String> {
    let mut projects = reports::projects(report_root, report_owner)?;
    if database.exists() {
        let store = crate::open_store(database)?;
        projects.extend(crate::projects_of_fence(&store, fence)?);
    }
    Ok(projects)
}

/// The one execution of the kernel's recovery control (`ds.sync-recovery/v1`)
/// over a host's store, under the fence the host captured. Status and preview
/// read and never create the store; retry and apply persist in the kernel's
/// one IMMEDIATE transaction. Every host answers the same result.
pub fn recover(
    database: &Path,
    fence: &crate::store::Fence,
    control: crate::store::recovery::Control,
    now_ms: u64,
) -> Result<crate::store::recovery::Result, String> {
    use crate::store::recovery;
    control.validate()?;
    let scope = crate::store::Scope::Project {
        project: control.project,
    };
    let write = matches!(
        &control.command,
        recovery::Command::Retry { .. } | recovery::Command::SanitizeApply { .. }
    );
    let store = if write {
        Some(ds_sync_store::Store::open(database).map_err(|error| error.to_string())?)
    } else {
        ds_sync_store::Store::open_read_only(database).map_err(|error| error.to_string())?
    };
    match store {
        Some(mut store) => store
            .recovery(fence, &scope, now_ms, control.command)
            .map_err(|error| error.to_string()),
        None => recovery::resolve(recovery::Request {
            schema: recovery::SCHEMA.into(),
            fence: fence.clone(),
            scope,
            now_ms,
            entries: Vec::new(),
            lease: None,
            command: control.command,
        })
        .map_err(|error| error.to_string()),
    }
}
