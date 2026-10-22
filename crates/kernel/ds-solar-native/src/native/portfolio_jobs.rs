//! Bounded, project-fenced portfolio workers and private restart receipts.
use super::*;
use std::io::Write;

const JOURNAL_SCHEMA: &str = "ds-solar.portfolio-job/v1";
const MAX_JOURNAL_BYTES: u64 = 16 << 20;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Running,
    Ready,
    Cancelled,
    Failed,
    Interrupted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Receipt {
    pub schema: String,
    pub project_id: String,
    pub root: String,
    pub job_id: String,
    pub workspace_handle: String,
    pub source_run_id: String,
    pub run_id: String,
    pub seq: u64,
    pub phase: Phase,
    pub done: bool,
    pub cancel_requested: bool,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
}

pub(crate) struct Entry {
    receipt: Receipt,
    cancel: CancelToken,
}

fn directory(app: &AppHandle, project: &str) -> Result<PathBuf, String> {
    checked_segment(project, "project id")?;
    secure_private_directory(&app.data_dir()?.join("solar-portfolio-jobs").join(project))
}

fn journal(app: &AppHandle, project: &str, id: &str) -> Result<PathBuf, String> {
    checked_segment(id, "portfolio job id")?;
    Ok(directory(app, project)?.join(format!("{id}.json")))
}

fn persist(app: &AppHandle, receipt: &Receipt) -> Result<(), String> {
    let path = journal(app, &receipt.project_id, &receipt.job_id)?;
    let bytes = json_bytes(receipt)?;
    if bytes.len() as u64 > MAX_JOURNAL_BYTES {
        return Err("Portfolio job receipt exceeds its bounded journal".into());
    }
    let temporary = path.with_extension(format!("{}.partial", uuid::Uuid::new_v4()));
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let write = (|| {
        let mut file = options
            .open(&temporary)
            .map_err(|_| "Cannot stage portfolio job receipt")?;
        file.write_all(&bytes)
            .map_err(|_| "Cannot write portfolio job receipt")?;
        file.sync_all()
            .map_err(|_| "Cannot flush portfolio job receipt")?;
        std::fs::rename(&temporary, &path).map_err(|_| "Cannot commit portfolio job receipt")
    })();
    if write.is_err() {
        let _ = std::fs::remove_file(temporary);
    }
    write.map_err(str::to_string)
}

fn read(app: &AppHandle, project: &str, id: &str) -> Result<Receipt, String> {
    let receipt: Receipt = serde_json::from_slice(&read_bounded_regular_file(
        &journal(app, project, id)?,
        "portfolio job receipt",
        MAX_JOURNAL_BYTES,
    )?)
    .map_err(|_| "Invalid portfolio job receipt")?;
    let terminal = matches!(
        receipt.phase,
        Phase::Ready | Phase::Cancelled | Phase::Failed | Phase::Interrupted
    );
    if receipt.schema != JOURNAL_SCHEMA
        || receipt.project_id != project
        || receipt.root != format!("eds_project/{project}/eds_solar")
        || receipt.job_id != id
        || receipt.done != terminal
        || (receipt.phase == Phase::Ready) != receipt.result.is_some()
    {
        return Err(
            "Portfolio job receipt belongs to another project or violates its state contract"
                .into(),
        );
    }
    checked_segment(&receipt.run_id, "portfolio run id")?;
    checked_segment(&receipt.source_run_id, "source run id")?;
    Ok(receipt)
}

/// Keep at most 64 journals. Active workers are never evicted.
fn retain(app: &AppHandle, project: &str, entries: &BTreeMap<String, Entry>) -> Result<(), String> {
    let mut files = Vec::new();
    for item in std::fs::read_dir(directory(app, project)?)
        .map_err(|_| "Cannot list portfolio job receipts")?
    {
        let item = item.map_err(|_| "Cannot inspect portfolio job receipts")?;
        let path = item.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let id = path
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid portfolio job filename")?;
        let receipt = read(app, project, id)?;
        let active = entries.get(id).is_some_and(|e| !e.receipt.done);
        let modified = item
            .metadata()
            .map_err(|_| "Cannot inspect portfolio receipt age")?
            .modified()
            .map_err(|_| "Cannot inspect portfolio receipt age")?;
        files.push((modified, path, receipt.done && !active));
    }
    files.sort_by_key(|entry| entry.0);
    while files.len() >= 64 {
        let position = files.iter().position(|entry| entry.2).ok_or(
            "Portfolio journal limit reached; inspect interrupted jobs before starting more",
        )?;
        let (_, path, _) = files.remove(position);
        std::fs::remove_file(path).map_err(|_| "Cannot expire terminal portfolio job receipt")?;
    }
    Ok(())
}

pub fn start(
    app: AppHandle,
    project: &str,
    command: PortfolioCalculateCommand,
) -> Result<Receipt, String> {
    checked_segment(&command.run_id, "portfolio run id")?;
    checked_segment(&command.source_run_id, "source run id")?;
    if command.project_id != project || command.root != format!("eds_project/{project}/eds_solar") {
        return Err("Portfolio worker belongs to another project".into());
    }
    resolve_workspace_handle(&app, &command.workspace_handle)?;
    let cancel = CancelToken::new();
    let engine = capture_portfolio_engine(&app, &cancel)?;
    let state = app.state();
    let mut jobs = state
        .portfolio_jobs
        .lock()
        .map_err(|_| "Portfolio worker state is poisoned")?;
    if jobs.values().filter(|e| !e.receipt.done).count() >= 32 {
        return Err("This native context already has 32 active portfolio jobs".into());
    }
    if jobs.values().any(|e| {
        !e.receipt.done && e.receipt.project_id == project && e.receipt.run_id == command.run_id
    }) {
        return Err("That portfolio run id already has an active worker".into());
    }
    while jobs.len() >= 32 {
        let key = jobs
            .iter()
            .find(|(_, e)| e.receipt.done)
            .map(|(id, _)| id.clone())
            .ok_or("This native context already has 32 active portfolio jobs")?;
        jobs.remove(&key);
    }
    retain(&app, project, &jobs)?;
    let id = uuid::Uuid::new_v4().to_string();
    let receipt = Receipt {
        schema: JOURNAL_SCHEMA.into(),
        project_id: project.into(),
        root: command.root.clone(),
        job_id: id.clone(),
        workspace_handle: command.workspace_handle.clone(),
        source_run_id: command.source_run_id.clone(),
        run_id: command.run_id.clone(),
        seq: state.next_seq(),
        phase: Phase::Queued,
        done: false,
        cancel_requested: false,
        result: None,
        error: None,
    };
    persist(&app, &receipt)?;
    jobs.insert(
        id.clone(),
        Entry {
            receipt: receipt.clone(),
            cancel: cancel.clone(),
        },
    );
    drop(jobs);
    let worker = app.clone();
    let worker_id = id.clone();
    let spawn = std::thread::Builder::new().name("solar-portfolio".into()).spawn(move || {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            update(&worker, &worker_id, |r| {r.phase=Phase::Running;})?;
            calculate_portfolio_with_cancel(worker.clone(), command, cancel.clone(), engine)
                .and_then(|report| serde_json::to_value(report).map_err(|_| "Cannot encode portfolio calculation receipt".into()))
        })).unwrap_or_else(|_| Err("Portfolio worker stopped before settlement; inspect its closed run before retrying".into()));
        let _ = update(&worker, &worker_id, |r| {
            r.done=true;
            r.cancel_requested |= cancel.is_cancelled();
            match outcome {
                Ok(result) => {r.phase=Phase::Ready;r.result=Some(result);}
                Err(error) => {
                    r.phase=if cancel.is_cancelled() && error.contains("(cancelled)") {Phase::Cancelled} else {Phase::Failed};
                    r.error=Some(error);
                }
            }
        });
        // A failed journal commit remains inspectable in live memory; restart
        // reports interruption rather than inventing a successful settlement.
    });
    if spawn.is_err() {
        update(&app, &id, |r| {
            r.done = true;
            r.phase = Phase::Failed;
            r.error = Some("Cannot start portfolio worker".into());
        })?;
        return Err("Cannot start portfolio worker".into());
    }
    Ok(receipt)
}

fn update(app: &AppHandle, id: &str, change: impl FnOnce(&mut Receipt)) -> Result<(), String> {
    let state = app.state();
    let mut jobs = state
        .portfolio_jobs
        .lock()
        .map_err(|_| "Portfolio worker state is poisoned")?;
    let entry = jobs
        .get_mut(id)
        .ok_or("Portfolio worker receipt was expired")?;
    change(&mut entry.receipt);
    entry.receipt.seq = state.next_seq();
    persist(app, &entry.receipt)
}

pub fn progress(app: &AppHandle, project: &str, id: &str) -> Result<Receipt, String> {
    let mut durable = read(app, project, id)?;
    let state = app.state();
    let jobs = state
        .portfolio_jobs
        .lock()
        .map_err(|_| "Portfolio worker state is poisoned")?;
    if let Some(entry) = jobs.get(id) {
        if entry.receipt.project_id != project {
            return Err("Portfolio worker belongs to another project".into());
        }
        return Ok(entry.receipt.clone());
    }
    if !durable.done {
        durable.done = true;
        durable.phase = Phase::Interrupted;
        durable.seq = durable.seq.saturating_add(1);
        durable.error=Some("Portfolio worker was interrupted. Inspect the closed run, then retry the same exact intent; a committed run is preserved.".into());
        persist(app, &durable)?;
    }
    Ok(durable)
}

pub fn cancel(app: &AppHandle, project: &str, id: &str) -> Result<bool, String> {
    // Authorize the durable project before touching any live cancellation token.
    read(app, project, id)?;
    let state = app.state();
    let mut jobs = state
        .portfolio_jobs
        .lock()
        .map_err(|_| "Portfolio worker state is poisoned")?;
    let Some(entry) = jobs.get_mut(id) else {
        return Ok(false);
    };
    if entry.receipt.project_id != project {
        return Err("Portfolio worker belongs to another project".into());
    }
    if entry.receipt.done {
        return Ok(false);
    }
    entry.cancel.cancel();
    entry.receipt.cancel_requested = true;
    entry.receipt.seq = state.next_seq();
    persist(app, &entry.receipt)?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn queued(app: &AppHandle) -> Receipt {
        let receipt = Receipt {
            schema: JOURNAL_SCHEMA.into(),
            project_id: "project_a".into(),
            root: "eds_project/project_a/eds_solar".into(),
            job_id: uuid::Uuid::new_v4().to_string(),
            workspace_handle: "solar-workspace-test".into(),
            source_run_id: "cities-source".into(),
            run_id: "portfolio-test".into(),
            seq: 1,
            phase: Phase::Queued,
            done: false,
            cancel_requested: false,
            result: None,
            error: None,
        };
        persist(app, &receipt).unwrap();
        receipt
    }

    #[test]
    fn restart_and_project_fences_preserve_run_identity() {
        let data = std::env::temp_dir().join(format!("ds-portfolio-jobs-{}", uuid::Uuid::new_v4()));
        let app = AppHandle::headless(data.clone()).unwrap();
        let receipt = queued(&app);
        assert!(progress(&app, "project_b", &receipt.job_id).is_err());
        assert!(cancel(&app, "project_b", &receipt.job_id).is_err());
        let restart = AppHandle::headless(data.clone()).unwrap();
        let interrupted = progress(&restart, "project_a", &receipt.job_id).unwrap();
        assert_eq!(interrupted.phase, Phase::Interrupted);
        assert!(interrupted.done);
        assert_eq!(interrupted.source_run_id, "cities-source");
        assert_eq!(interrupted.run_id, "portfolio-test");
        assert!(interrupted.result.is_none());
        assert!(!cancel(&restart, "project_a", &receipt.job_id).unwrap());
        assert_eq!(
            progress(&restart, "project_a", &receipt.job_id)
                .unwrap()
                .seq,
            interrupted.seq
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(journal(&restart, "project_a", &receipt.job_id).unwrap())
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
        std::fs::remove_dir_all(data).unwrap();
    }

    #[test]
    fn cancellation_request_does_not_claim_a_completed_run_was_cancelled() {
        let data =
            std::env::temp_dir().join(format!("ds-portfolio-cancel-{}", uuid::Uuid::new_v4()));
        let app = AppHandle::headless(data.clone()).unwrap();
        let receipt = queued(&app);
        let token = CancelToken::new();
        app.state().portfolio_jobs.lock().unwrap().insert(
            receipt.job_id.clone(),
            Entry {
                receipt: receipt.clone(),
                cancel: token.clone(),
            },
        );
        assert!(cancel(&app, "project_a", &receipt.job_id).unwrap());
        assert!(token.is_cancelled());
        let requested = progress(&app, "project_a", &receipt.job_id).unwrap();
        assert!(requested.cancel_requested);
        assert!(!requested.done);
        assert_eq!(requested.phase, Phase::Queued);
        update(&app, &receipt.job_id, |r| {
            r.phase = Phase::Ready;
            r.done = true;
            r.result = Some(serde_json::json!({"run_id":"portfolio-test"}));
        })
        .unwrap();
        assert!(!cancel(&app, "project_a", &receipt.job_id).unwrap());
        assert_eq!(
            progress(&app, "project_a", &receipt.job_id).unwrap().phase,
            Phase::Ready
        );
        std::fs::remove_dir_all(data).unwrap();
    }
}
