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
