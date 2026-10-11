//! Native RAM jobs. Solving owns a private fork, never the live runtime lock.
//! Only explicit, head-fenced admission can replace the calculated review.
use super::*;
use ds_grid_engine::SessionHistoryState;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, TryRecvError};
use std::time::Instant;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StartCalculationRequest {
    pub job_id: String,
    pub request: RefreshRequest,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationJobRequest {
    pub scope: String,
    pub model_id: String,
    pub job_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AdmitCalculationRequest {
    pub scope: String,
    pub model_id: String,
    pub job_id: String,
    /// Required for asynchronous admission; the usual confirm/discard follows.
    pub publication_id: Option<String>,
}

struct Completed {
    held: HeldView,
    state: DisplayState,
    analysis: Option<Value>,
    result: Value,
}

struct Job {
    scope: String,
    model_id: String,
    history: SessionHistoryState,
    context: Arc<()>,
    publication_id: String,
    started: Instant,
    start_ms: f64,
    cancel: Arc<AtomicBool>,
    receiver: Receiver<Result<Completed, String>>,
    ready: Option<Result<Completed, String>>,
    stale: bool,
}

#[derive(Default)]
pub(super) struct Jobs {
    entries: BTreeMap<String, Job>,
    used_ids: std::collections::BTreeSet<String>,
    // Cancelled noninterruptible work still occupies this slot until it exits.
    active: Arc<AtomicUsize>,
    #[cfg(test)]
    gate: Option<(Receiver<()>, mpsc::Sender<()>)>,
}

impl Jobs {
    pub(super) fn release_scope(&mut self, scope: &str) {
        self.entries.retain(|_, job| {
            if job.scope == scope {
                job.cancel.store(true, Ordering::Release);
                false
            } else {
                true
            }
        });
    }

    #[cfg(test)]
    pub(super) fn pause_next(&mut self) -> (mpsc::Sender<()>, Receiver<()>) {
        let (release, gate) = mpsc::channel();
        let (entered, waiting) = mpsc::channel();
        self.gate = Some((gate, entered));
        (release, waiting)
    }
}

impl Drop for Jobs {
    fn drop(&mut self) {
        for job in self.entries.values() {
            job.cancel.store(true, Ordering::Release);
        }
    }
}

impl Job {
    fn poll(&mut self) {
        if self.cancel.load(Ordering::Acquire) {
            // A result sent concurrently with cancellation is discarded too.
            let _ = self.receiver.try_recv();
            self.ready = Some(Err(if self.stale {
                "profile_calculation_stale: result discarded".into()
            } else {
                "profile_calculation_cancelled: result discarded".into()
            }));
            return;
        }
        if self.ready.is_none() {
            match self.receiver.try_recv() {
                Ok(value) => self.ready = Some(value),
                Err(TryRecvError::Disconnected) => {
                    self.ready =
                        Some(Err("profile_calculation_failed: worker disconnected".into()));
                }
                Err(TryRecvError::Empty) => {}
            }
        }
    }

    fn receipt(&self, id: &str, active: bool) -> Value {
        let status = if self.stale {
            "stale"
        } else if self.cancel.load(Ordering::Acquire) {
            "cancelled"
        } else {
            match &self.ready {
                None => "running",
                Some(Ok(_)) => "ready",
                Some(Err(_)) => "failed",
            }
        };
        json!({"job_id":id,"scope":self.scope,"model_id":self.model_id,
            "history":self.history,"status":status,"solver_interruptible":false,
            "runtime_worker_active":active,"start_ms":self.start_ms,
            "elapsed_ms":self.started.elapsed().as_secs_f64()*1000.0,
            "error":self.ready.as_ref().and_then(|value|value.as_ref().err())
                .map(|error|error.chars().take(1024).collect::<String>())})
    }
}

struct ActiveWorker(Arc<AtomicUsize>);
impl Drop for ActiveWorker {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::AcqRel);
    }
}

pub(super) fn identity(id: &str) -> Result<(), String> {
    if id.is_empty() || id.len() > 128 || id.chars().any(char::is_control) {
        return Err("invalid_profile_view: invalid calculation identity".into());
    }
    Ok(())
}

impl Runtime {
    fn job_current(&self, job: &Job) -> bool {
        self.held.get(&job.scope).is_some_and(|held| {
            held.source.model_id == job.model_id && held.session.history_state() == job.history
        }) && self
            .calculation_contexts
            .get(&job.scope)
            .is_some_and(|context| Arc::ptr_eq(context, &job.context))
    }

    pub(super) fn start_calculation(
        &mut self,
        input: StartCalculationRequest,
    ) -> Result<Value, String> {
        self.start_calculation_with_transfer(input, None, false)
    }

    pub(super) fn start_calculation_with_transfer(
        &mut self,
        input: StartCalculationRequest,
        base_signature: Option<String>,
        exact_json: bool,
    ) -> Result<Value, String> {
        let started = Instant::now();
        identity(&input.job_id)?;
        let mut request = input.request;
        if request.journal_json.len() > 64 * 1024 * 1024 {
            return Err("profile_replay_invalid: journal exceeds native transport bounds".into());
        }
        let publication_id = request
            .publication_id
            .take()
            .ok_or("invalid_profile_view: a calculation job requires a publication identity")?;
        identity(&publication_id)?;
        if self.calculation_jobs.used_ids.contains(&input.job_id) {
            return Err("profile_calculation_stale: a job identity cannot be reused".into());
        }
        if self.calculation_jobs.used_ids.len() >= 16_384 {
            return Err(
                "profile_calculation_capacity: native job identity capacity reached".into(),
            );
        }
        if self
            .publications
            .get(&request.scope)
            .is_some_and(|pending| !pending.confirmed)
        {
            return Err("profile_publication_pending: admit the outstanding native result before starting calculation".into());
        }
        if self
            .publications
            .get(&request.scope)
            .is_some_and(|pending| pending.pin.publication_id == publication_id)
        {
            return Err(
                "profile_publication_stale: a publication identity cannot be reused".into(),
            );
        }
        let held = self
            .held
            .get(&request.scope)
            .ok_or("profile_replay_base_unavailable: initialize this model scope first")?;
        let history = held.session.history_state();
        if held.source.model_id != request.model_id {
            return Err(
                "profile_replay_model_mismatch: calculation belongs to another model".into(),
            );
        }
        if request.base_history != history
            || request.target_history != history
            || request.base_revision != history.model_revision
            || request.expected_revision != history.model_revision
        {
            return Err("profile_replay_stale: observe the exact authored history before starting calculation".into());
        }
        let context = self
            .calculation_contexts
            .get(&request.scope)
            .ok_or("profile_calculation_stale: native calculation context is missing")?
            .clone();
        if self.calculation_jobs.active.load(Ordering::Acquire) != 0 {
            return Err(
                "profile_calculation_busy: another native calculation worker is still running"
                    .into(),
            );
        }
        self.calculation_jobs.entries.retain(|_, job| {
            job.poll();
            !(job.ready.is_some() && (job.stale || job.cancel.load(Ordering::Acquire)))
        });
        if self.calculation_jobs.entries.len() >= 8 {
            return Err(
                "profile_calculation_capacity: admit or cancel an unused calculation result".into(),
            );
        }
        if self
            .calculation_jobs
            .active
            .compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return Err(
                "profile_calculation_busy: another native calculation worker is still running"
                    .into(),
            );
        }
        let worker_slot = ActiveWorker(self.calculation_jobs.active.clone());
        let mut fork = Runtime::default();
        if exact_json {
            fork.exact_transfers.insert(request.scope.clone());
        }
        if let Some(signature) = base_signature {
            fork.delta_transfers
                .insert(request.scope.clone(), signature);
        }
        fork.held.insert(request.scope.clone(), held.clone());
        fork.views
            .insert(request.scope.clone(), self.views[&request.scope].clone());
        if let Some(analysis) = self.analyses.get(&request.scope) {
            fork.analyses
                .insert(request.scope.clone(), analysis.clone());
        }
        let scope = request.scope.clone();
        let job_scope = scope.clone();
        let model_id = request.model_id.clone();
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (sender, receiver) = mpsc::channel();
        #[cfg(test)]
        let gate = self.calculation_jobs.gate.take();
        std::thread::Builder::new()
            .name("ds-profile-calculation".into())
            .spawn(move || {
                let _slot = worker_slot;
                #[cfg(test)]
                if let Some((gate, entered)) = gate {
                    let _ = entered.send(());
                    let _ = gate.recv();
                }
                let calculate = || -> Result<Completed, String> {
                    if worker_cancel.load(Ordering::Acquire) {
                        return Err("profile_calculation_cancelled: result discarded".into());
                    }
                    let worker_started = Instant::now();
                    let mut result = fork.execute(Request::Refresh(Box::new(request)))?;
                    if worker_cancel.load(Ordering::Acquire) {
                        return Err("profile_calculation_cancelled: result discarded".into());
                    }
                    result["metrics"]["job_worker_ms"] =
                        json!(worker_started.elapsed().as_secs_f64() * 1000.0);
                    Ok(Completed {
                        held: fork.held.remove(&scope).expect("calculated held view"),
                        state: fork.views.remove(&scope).expect("calculated display"),
                        analysis: fork.analyses.remove(&scope),
                        result,
                    })
                };
                let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(calculate))
                    .unwrap_or_else(|_| {
                        Err("profile_calculation_failed: native worker panicked".into())
                    });
                drop(_slot);
                let _ = sender.send(value);
            })
            .map_err(|error| {
                format!("profile_calculation_failed: cannot start native worker: {error}")
            })?;
        let job = Job {
            scope: job_scope,
            model_id,
            history,
            context,
            publication_id,
            started,
            start_ms: started.elapsed().as_secs_f64() * 1000.0,
            cancel,
            receiver,
            ready: None,
            stale: false,
        };
        let receipt = job.receipt(&input.job_id, true);
        self.calculation_jobs.used_ids.insert(input.job_id.clone());
        self.calculation_jobs.entries.insert(input.job_id, job);
        Ok(receipt)
    }

    pub(super) fn calculation_publication(
        &self,
        request: &CalculationJobRequest,
    ) -> Result<String, String> {
        Ok(self.lookup_job(request)?.publication_id.clone())
    }

    fn lookup_job(&self, request: &CalculationJobRequest) -> Result<&Job, String> {
        identity(&request.job_id)?;
        if request.scope.len() > 1024 || request.model_id.len() > 200 {
            return Err("invalid_profile_view: invalid calculation scope or model".into());
        }
        let job = self
            .calculation_jobs
            .entries
            .get(&request.job_id)
            .ok_or("profile_calculation_unavailable: no calculation job exists")?;
        if request.scope != job.scope || request.model_id != job.model_id {
            return Err(
                "profile_calculation_stale: calculation belongs to another model scope".into(),
            );
        }
        Ok(job)
    }

    pub(super) fn calculation_status(
        &mut self,
        request: CalculationJobRequest,
    ) -> Result<Value, String> {
        let current = self.job_current(self.lookup_job(&request)?);
        let active = self.calculation_jobs.active.load(Ordering::Acquire) != 0;
        let job = self
            .calculation_jobs
            .entries
            .get_mut(&request.job_id)
            .unwrap();
        if !current {
            job.stale = true;
            job.cancel.store(true, Ordering::Release);
        }
        job.poll();
        if job.stale {
            job.ready = Some(Err("profile_calculation_stale: result discarded".into()));
        }
        Ok(job.receipt(&request.job_id, active))
    }

    pub(super) fn cancel_calculation(
        &mut self,
        request: CalculationJobRequest,
    ) -> Result<Value, String> {
        self.lookup_job(&request)?;
        let active = self.calculation_jobs.active.load(Ordering::Acquire) != 0;
        let job = self
            .calculation_jobs
            .entries
            .get_mut(&request.job_id)
            .unwrap();
        job.cancel.store(true, Ordering::Release);
        job.poll();
        // Drop a completed dense result now; a running worker checks the same
        // flag before and after its noninterruptible solve.
        job.ready = Some(Err("profile_calculation_cancelled: result discarded".into()));
        Ok(job.receipt(&request.job_id, active))
    }

    pub(super) fn admit_calculation(
        &mut self,
        request: AdmitCalculationRequest,
    ) -> Result<Value, String> {
        let reference = CalculationJobRequest {
            scope: request.scope.clone(),
            model_id: request.model_id,
            job_id: request.job_id.clone(),
        };
        let job = self.lookup_job(&reference)?;
        if !self.job_current(job) || job.stale {
            return Err(
                "profile_calculation_stale: authored history or calculation context changed".into(),
            );
        }
        // Context invalidation also stops the worker. Preserve that stale
        // refusal, while distinguishing an explicit cancellation at this head.
        if job.cancel.load(Ordering::Acquire) {
            return Err("profile_calculation_cancelled: the result was cancelled".into());
        }
        if request.publication_id.as_ref() != Some(&job.publication_id) {
            return Err(
                "profile_publication_stale: calculation publication differs from its start request"
                    .into(),
            );
        }
        let job = self
            .calculation_jobs
            .entries
            .get_mut(&request.job_id)
            .unwrap();
        job.poll();
        let Some(value) = job.ready.take() else {
            return Err("profile_calculation_pending: calculation is still running".into());
        };
        let completed = match value {
            Ok(value) => value,
            Err(error) => {
                job.ready = Some(Err(error.clone()));
                return Err(error);
            }
        };
        let start_ms = job.start_ms;
        let elapsed_ms = job.started.elapsed().as_secs_f64() * 1000.0;
        self.calculation_jobs.entries.remove(&request.job_id);
        self.held.insert(request.scope.clone(), completed.held);
        self.views.insert(request.scope.clone(), completed.state);
        if let Some(analysis) = completed.analysis {
            self.analyses.insert(request.scope.clone(), analysis);
        } else {
            self.analyses.remove(&request.scope);
        }
        let mut result = completed.result;
        result["metrics"]["job_start_ms"] = json!(start_ms);
        result["metrics"]["job_elapsed_ms"] = json!(elapsed_ms);
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactional_projection_tests::{describe, refresh, request_for};
    use std::time::Duration;

    fn fixture() -> ds_grid_model::GridModelSnapshot {
        use ds_grid_model::*;
        let mut snapshot = common::fixture();
        for (index, (x_m, y_m, z_m)) in
            [(0.0, 0.0, 100.0), (300.0, 0.0, 102.0), (600.0, 40.0, 98.0)]
                .into_iter()
                .enumerate()
        {
            snapshot.terrain_points.push(TerrainPointRow {
                id: TerrainPointId::new(format!("job-basis-{index}")).unwrap(),
                x_m,
                y_m,
                z_m,
                feature_class: "ground".into(),
                description: None,
                required_clearance_m: None,
                source_id: None,
            });
        }
        snapshot.tension_sections.push(TensionSectionRow {
            id: TensionSectionId::new("job-section").unwrap(),
            cable_id: CableId::new("cb-1").unwrap(),
            criterion_set_id: None,
            sag: AuthoredSag::HorizontalTension {
                tension_n: 20_000.0,
            },
            sag_condition: Some(CableCondition::Initial),
            sag_weather_state_id: Some(WeatherStateId::new("ws-everyday").unwrap()),
            ruling_span_m: Some(300.0),
            details: Default::default(),
        });
        for (sequence, id) in ["str-1", "str-2", "str-3"].into_iter().enumerate() {
            snapshot
                .tension_section_supports
                .push(TensionSectionSupportRow {
                    section_id: TensionSectionId::new("job-section").unwrap(),
                    sequence: sequence as u32,
                    structure_id: StructureId::new(id).unwrap(),
                    attachment_point_id: Some(AttachmentPointId::new("ap-left").unwrap()),
                    insulator_model: None,
                });
        }
        snapshot
    }
    fn request() -> FixtureInput {
        request_for(fixture())
    }

    fn reference(id: &str) -> CalculationJobRequest {
        CalculationJobRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            job_id: id.into(),
        }
    }
    fn start(
        runtime: &mut Runtime,
        input: &FixtureInput,
        worker: &GridSession,
        id: &str,
        analyze: bool,
    ) -> Value {
        let base = runtime.held[&input.scope]
            .session
            .current_revision()
            .revision_id
            .to_string();
        let mut request = refresh(runtime, input, &base, worker);
        request.publication_id = Some(format!("publication-{id}"));
        request.analyze = analyze;
        let mut wire = serde_json::to_value(StartCalculationRequest {
            job_id: id.into(),
            request,
        })
        .unwrap();
        wire["operation"] = json!("start_calculation");
        runtime
            .execute(serde_json::from_value::<Request>(wire).unwrap())
            .unwrap()
    }
    fn admit(id: &str) -> Request {
        Request::AdmitCalculation(AdmitCalculationRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            job_id: id.into(),
            publication_id: Some(format!("publication-{id}")),
        })
    }
    fn wait_ready(runtime: &mut Runtime, id: &str) -> Value {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let status = runtime
                .execute(Request::CalculationStatus(reference(id)))
                .unwrap();
            assert!(serde_json::to_vec(&status).unwrap().len() < 2048);
            if status["status"] != "running" {
                return status;
            }
            assert!(
                Instant::now() < deadline,
                "native calculation did not finish"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn authored_observation_and_native_selection_continue_before_a_calculation_finishes() {
        let input = request();
        let mut runtime = Runtime::default();
        let initial = runtime.execute(input.clone()).unwrap();
        let mut worker = GridSession::open(fixture());
        let (release, entered) = runtime.calculation_jobs.pause_next();
        assert_eq!(
            start(&mut runtime, &input, &worker, "concurrent", true)["status"],
            "running"
        );
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let pending = runtime.execute(admit("concurrent")).unwrap_err();
        assert!(pending.starts_with("profile_calculation_pending:"));
        describe(&mut worker, "during-job", "still editable");
        let observation = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        assert_eq!(observation["scene"], Value::Null);
        assert_eq!(observation["analysis"], Value::Null);
        assert!(observation["authored_delta"].is_object());
        assert_eq!(observation["history"], json!(worker.history_state()));
        assert_eq!(
            observation["calculation"]["computed_revision"],
            initial["model_revision"]
        );
        let selection: Request = serde_json::from_value(
            json!({"operation":"selection_controls","scope":"one","space":"profile","available_families":["structures","terrain_points"],"request":{"action":"read"}}),
        )
        .unwrap();
        runtime.execute(selection).unwrap();
        assert_eq!(
            runtime
                .execute(Request::CalculationStatus(reference("concurrent")))
                .unwrap()["status"],
            "stale"
        );
        assert!(runtime
            .execute(admit("concurrent"))
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
        assert_eq!(
            runtime.held["one"].session.history_state(),
            worker.history_state()
        );
        assert_eq!(
            runtime.held["one"]
                .review
                .base_scene()
                .unwrap()
                .model_revision
                .to_string(),
            input.expected_revision
        );
        release.send(()).unwrap();
    }

    #[test]
    fn calculated_result_matches_fresh_and_uses_the_existing_confirm_and_discard_boundary() {
        let input = request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut worker = GridSession::open(fixture());
        describe(&mut worker, "edit", "changed");
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        let computed_before = runtime.held["one"]
            .review
            .base_scene()
            .unwrap()
            .model_revision
            .clone();
        let ready = start(&mut runtime, &input, &worker, "parity", true);
        assert!(serde_json::to_vec(&ready).unwrap().len() < 2048);
        assert_eq!(wait_ready(&mut runtime, "parity")["status"], "ready");
        // Status never publishes calculated results or clears pending edits.
        assert_eq!(
            runtime.held["one"]
                .review
                .base_scene()
                .unwrap()
                .model_revision,
            computed_before
        );
        assert!(!runtime.held["one"].pending_receipts.is_empty());
        let mut synchronous = Runtime::default();
        synchronous.execute(input.clone()).unwrap();
        let mut sync_request = refresh(&synchronous, &input, &input.expected_revision, &worker);
        sync_request.analyze = true;
        let expected = synchronous.execute(sync_request).unwrap();
        let result = runtime.execute(admit("parity")).unwrap();
        assert_eq!(result["scene"], expected["scene"]);
        assert_eq!(result["analysis"], expected["analysis"]);
        assert_eq!(result["command_view"], expected["command_view"]);
        let mut fresh_request = request_for(worker.snapshot().clone());
        fresh_request.analyze = true;
        fresh_request.patch.visibility.insert(
            ds_command_kernel::profile_display::DisplayOption::Review,
            true,
        );
        let fresh = Runtime::default().execute(fresh_request).unwrap();
        assert_eq!(result["scene"], fresh["scene"]);
        assert_eq!(result["analysis"], fresh["analysis"]);
        assert_eq!(result["history"], json!(worker.history_state()));
        assert!(result["metrics"]["job_start_ms"].as_f64().unwrap() >= 0.0);
        let pin: PublicationPin = serde_json::from_value(result["publication"].clone()).unwrap();
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: "one".into(),
                publication: pin.clone(),
            }))
            .unwrap();
        runtime
            .execute(Request::DiscardPublication(DiscardPublicationRequest {
                scope: "one".into(),
                model_id: pin.model_id,
                publication_id: pin.publication_id,
                target_history: pin.history,
            }))
            .unwrap();
        assert_eq!(
            runtime.held["one"].session.history_state(),
            worker.history_state()
        );
        assert_eq!(
            runtime.held["one"]
                .review
                .base_scene()
                .unwrap()
                .model_revision,
            computed_before
        );
        assert!(!runtime.held["one"].pending_receipts.is_empty());
        assert_eq!(
            start(&mut runtime, &input, &worker, "retry", false)["status"],
            "running"
        );
        assert_eq!(wait_ready(&mut runtime, "retry")["status"], "ready");
        runtime.execute(admit("retry")).unwrap();
    }

    #[test]
    fn cancellation_keeps_the_physical_worker_slot_and_interleaved_scopes_isolated() {
        let input = request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let worker = GridSession::open(fixture());
        let (release, entered) = runtime.calculation_jobs.pause_next();
        start(&mut runtime, &input, &worker, "cancel", false);
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let cancelled = runtime
            .execute(Request::CancelCalculation(reference("cancel")))
            .unwrap();
        assert_eq!(cancelled["status"], "cancelled");
        assert_eq!(cancelled["solver_interruptible"], false);
        assert_eq!(cancelled["runtime_worker_active"], true);
        let mut request = refresh(&runtime, &input, &input.expected_revision, &worker);
        request.publication_id = Some("next-publication".into());
        assert!(runtime
            .execute(Request::StartCalculation(Box::new(
                StartCalculationRequest {
                    job_id: "next".into(),
                    request
                }
            )))
            .unwrap_err()
            .starts_with("profile_calculation_busy:"));
        assert!(runtime
            .execute(admit("cancel"))
            .unwrap_err()
            .starts_with("profile_calculation_cancelled:"));
        let mut other = input.clone();
        other.scope = "two".into();
        runtime.execute(other).unwrap();
        let mut wrong = reference("cancel");
        wrong.scope = "two".into();
        assert!(runtime
            .execute(Request::CalculationStatus(wrong))
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
        let mut wrong_admit = match admit("cancel") {
            Request::AdmitCalculation(request) => request,
            _ => unreachable!(),
        };
        wrong_admit.scope = "two".into();
        assert!(runtime
            .execute(Request::AdmitCalculation(wrong_admit))
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
        assert_eq!(
            runtime
                .execute(Request::CalculationStatus(reference("cancel")))
                .unwrap()["status"],
            "cancelled"
        );
        assert_eq!(
            runtime.held["one"].session.history_state(),
            worker.history_state()
        );
        release.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let status = runtime
                .execute(Request::CalculationStatus(reference("cancel")))
                .unwrap();
            assert_eq!(status["status"], "cancelled");
            if status["runtime_worker_active"] == false {
                break;
            }
            assert!(
                Instant::now() < until,
                "cancelled worker did not release its real slot"
            );
            std::thread::sleep(Duration::from_millis(2));
        }
        start(&mut runtime, &input, &worker, "recovered", false);
        assert_eq!(wait_ready(&mut runtime, "recovered")["status"], "ready");
        let recovered = runtime.execute(admit("recovered")).unwrap();
        runtime
            .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                scope: "one".into(),
                publication: serde_json::from_value(recovered["publication"].clone()).unwrap(),
            }))
            .unwrap();
        let fresh = Runtime::default().execute(input).unwrap();
        assert_eq!(recovered["scene"], fresh["scene"]);
        assert_eq!(
            runtime.held["one"].session.history_state(),
            worker.history_state()
        );
    }

    #[test]
    fn model_and_display_aba_release_and_failed_calculation_cannot_publish() {
        let input = request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut worker = GridSession::open(fixture());
        let captured = worker.history_state();
        start(&mut runtime, &input, &worker, "history-aba", false);
        assert_eq!(wait_ready(&mut runtime, "history-aba")["status"], "ready");
        describe(&mut worker, "away", "temporary");
        let away = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        worker.undo().unwrap();
        let away_revision = away["model_revision"].as_str().unwrap();
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                away_revision,
                &worker,
            ))))
            .unwrap();
        assert_eq!(
            worker.history_state().model_revision,
            captured.model_revision
        );
        assert_ne!(worker.history_state(), captured);
        assert!(runtime
            .execute(admit("history-aba"))
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
        start(&mut runtime, &input, &worker, "display-aba", false);
        assert_eq!(wait_ready(&mut runtime, "display-aba")["status"], "ready");
        let baseline = runtime.views["one"].clone();
        for on in [false, true] {
            let mut changed = refresh(&runtime, &input, &input.expected_revision, &worker);
            changed.patch.visibility.insert(
                ds_command_kernel::profile_display::DisplayOption::Ground,
                on,
            );
            runtime.execute(changed).unwrap();
        }
        assert_eq!(runtime.views["one"], baseline);
        assert!(runtime
            .execute(admit("display-aba"))
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
        start(&mut runtime, &input, &worker, "release", false);
        assert_eq!(wait_ready(&mut runtime, "release")["status"], "ready");
        runtime
            .execute(ReleaseRequest {
                scope: "one".into(),
                model_id: "profile-test".into(),
            })
            .unwrap();
        runtime.execute(input.clone()).unwrap();
        assert!(runtime
            .execute(admit("release"))
            .unwrap_err()
            .starts_with("profile_calculation_unavailable:"));
        let worker = GridSession::open(fixture());
        let mut invalid = refresh(&runtime, &input, &input.expected_revision, &worker);
        invalid.publication_id = Some("failed-publication".into());
        invalid.journal_json = "malformed".into();
        runtime
            .execute(Request::StartCalculation(Box::new(
                StartCalculationRequest {
                    job_id: "failed".into(),
                    request: invalid,
                },
            )))
            .unwrap();
        let status = wait_ready(&mut runtime, "failed");
        assert_eq!(status["status"], "failed");
        assert!(status["error"]
            .as_str()
            .unwrap()
            .starts_with("profile_replay_invalid:"));
        let invalid_admit = Request::AdmitCalculation(AdmitCalculationRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            job_id: "failed".into(),
            publication_id: Some("failed-publication".into()),
        });
        assert!(runtime
            .execute(invalid_admit)
            .unwrap_err()
            .starts_with("profile_replay_invalid:"));
        assert_eq!(
            runtime.held["one"].session.history_state(),
            worker.history_state()
        );
        assert!(!runtime.publications.contains_key("one"));
    }

    #[test]
    fn native_job_json_refuses_unobserved_heads_foreign_models_and_wrong_publication() {
        let input = request();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let mut worker = GridSession::open(fixture());
        describe(&mut worker, "unobserved", "edited");
        let mut changed = refresh(&runtime, &input, &input.expected_revision, &worker);
        changed.publication_id = Some("future-publication".into());
        assert!(runtime
            .execute(Request::StartCalculation(Box::new(
                StartCalculationRequest {
                    job_id: "future".into(),
                    request: changed
                }
            )))
            .unwrap_err()
            .starts_with("profile_replay_stale:"));
        runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &input.expected_revision,
                &worker,
            ))))
            .unwrap();
        let base = worker.current_revision().revision_id.to_string();
        let mut foreign = refresh(&runtime, &input, &base, &worker);
        foreign.publication_id = Some("foreign-publication".into());
        foreign.model_id = "foreign".into();
        assert!(runtime
            .execute(Request::StartCalculation(Box::new(
                StartCalculationRequest {
                    job_id: "foreign".into(),
                    request: foreign
                }
            )))
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        start(&mut runtime, &input, &worker, "wire", false);
        let status: Request = serde_json::from_value(json!({"operation":"calculation_status","scope":"one","model_id":"profile-test","job_id":"wire"})).unwrap();
        assert!(runtime.execute(status).unwrap()["history"].is_object());
        assert_eq!(wait_ready(&mut runtime, "wire")["status"], "ready");
        let wrong: Request = serde_json::from_value(json!({"operation":"admit_calculation","scope":"one","model_id":"profile-test","job_id":"wire","publication_id":"wrong"})).unwrap();
        assert!(runtime
            .execute(wrong)
            .unwrap_err()
            .starts_with("profile_publication_stale:"));
        assert!(!runtime.publications.contains_key("one"));
        let valid: Request = serde_json::from_value(json!({"operation":"admit_calculation","scope":"one","model_id":"profile-test","job_id":"wire","publication_id":"publication-wire"})).unwrap();
        runtime.execute(valid).unwrap();
        assert!(runtime
            .execute(Request::CalculationStatus(reference("wire")))
            .unwrap_err()
            .starts_with("profile_calculation_unavailable:"));
    }

    #[test]
    #[ignore = "bounded native job cost measurement; invoke explicitly with --ignored --nocapture"]
    fn dense_native_job_costs_and_fresh_parity() {
        use ds_grid_model::{StructureId, TerrainPointId, TerrainPointRow};
        let mut snapshot = fixture();
        for index in 0..10_000 {
            let x_m = 600.0 * index as f64 / 9_999.0;
            snapshot.terrain_points.push(TerrainPointRow {
                id: TerrainPointId::new(format!("job-ground-{index}")).unwrap(),
                x_m,
                y_m: (x_m - 300.0).max(0.0) * 40.0 / 300.0,
                z_m: 100.0 + (x_m / 30.0).sin(),
                feature_class: "ground".into(),
                description: None,
                required_clearance_m: None,
                source_id: None,
            });
        }
        for index in 0..400 {
            let mut row = snapshot.structures[1].clone();
            row.id = StructureId::new(format!("job-support-{index}")).unwrap();
            row.station_m = Some(600.0 * (index + 1) as f64 / 401.0);
            row.x_m = row.station_m.unwrap();
            row.y_m = (row.x_m - 300.0).max(0.0) * 40.0 / 300.0;
            row.z_m = 100.0 + (row.x_m / 30.0).sin();
            snapshot.structures.push(row);
        }
        let input = request_for(snapshot.clone());
        let mut runtime = Runtime::default();
        let initial = runtime.execute(input.clone()).unwrap();
        let scene_bytes = serde_json::to_vec(&initial["scene"]).unwrap().len();
        let mut worker = GridSession::open(snapshot);
        let mut samples = Vec::new();
        for index in 0..12 {
            let base = worker.current_revision().revision_id.to_string();
            let edit_started = Instant::now();
            describe(
                &mut worker,
                &format!("dense-edit-{index}"),
                if index % 2 == 0 { "B" } else { "A" },
            );
            let apply_ms = edit_started.elapsed().as_secs_f64() * 1000.0;
            let observe_started = Instant::now();
            let observation = runtime
                .execute(Request::Observe(Box::new(refresh(
                    &runtime, &input, &base, &worker,
                ))))
                .unwrap();
            let observe_ms = observe_started.elapsed().as_secs_f64() * 1000.0;
            let id = format!("dense-job-{index}");
            let started = start(&mut runtime, &input, &worker, &id, true);
            assert_eq!(wait_ready(&mut runtime, &id)["status"], "ready");
            let admission_started = Instant::now();
            let result = runtime.execute(admit(&id)).unwrap();
            let admit_ms = admission_started.elapsed().as_secs_f64() * 1000.0;
            let result_bytes = serde_json::to_vec(&result).unwrap().len();
            let mut fresh_input = request_for(worker.snapshot().clone());
            fresh_input.analyze = true;
            fresh_input.patch.visibility.insert(
                ds_command_kernel::profile_display::DisplayOption::Review,
                true,
            );
            let fresh = Runtime::default().execute(fresh_input).unwrap();
            assert_eq!(result["scene"], fresh["scene"]);
            assert_eq!(result["analysis"], fresh["analysis"]);
            let pin: PublicationPin =
                serde_json::from_value(result["publication"].clone()).unwrap();
            runtime
                .execute(Request::ConfirmPublication(ConfirmPublicationRequest {
                    scope: "one".into(),
                    publication: pin,
                }))
                .unwrap();
            samples.push(json!({"apply_ms":apply_ms,"observe_ms":observe_ms,"observe_metrics":observation["metrics"],
                "start_ms":started["start_ms"],"admit_ms":admit_ms,"calculation_metrics":result["metrics"],"result_bytes":result_bytes}));
        }
        let receipt = json!({"profile":if cfg!(debug_assertions){"native_debug"}else{"native_release"},"terrain_points":worker.snapshot().terrain_points.len(),
            "structures":worker.snapshot().structures.len(),"tension_sections":worker.snapshot().tension_sections.len(),"scene_bytes":scene_bytes,
            "samples":samples,"fresh_scene_and_analysis_parity":true,"excludes":["host transport","worker picking","renderer paint","receipt serialization"]});
        if let Some(path) = std::env::var_os("DS_PROFILE_JOB_COST_RECEIPT") {
            std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        }
        println!("{}", serde_json::to_string(&receipt).unwrap());
    }
}
