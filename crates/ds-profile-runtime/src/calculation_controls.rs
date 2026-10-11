//! Public bounded job controls over a captured native model scope. Hosts supply
//! identity/context; Rust captures calculation inputs and publication ownership.
use super::*;
use ds_command_kernel::profile_display::DisplayCase;
use ds_grid_engine::SessionHistoryState;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationTransfer {
    Full {},
    ExactJson {},
    Delta { base_signature: String },
}
impl Default for CalculationTransfer {
    fn default() -> Self {
        Self::Full {}
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case", deny_unknown_fields)]
pub enum CalculationArguments {
    Start {
        job_id: String,
        #[serde(default)]
        display_case: Option<DisplayCase>,
        #[serde(default)]
        analyze: bool,
        #[serde(default)]
        scene_transfer: CalculationTransfer,
    },
    PrepareSceneBase {},
    Status {
        job_id: String,
    },
    Cancel {
        job_id: String,
    },
    Admit {
        job_id: String,
    },
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::transactional_projection_tests::{
        describe, refresh, request_for, section_demand_fixture,
    };
    use std::time::{Duration, Instant};

    fn input() -> FixtureInput {
        request_for(section_demand_fixture())
    }
    fn control(
        history: Option<SessionHistoryState>,
        request: CalculationArguments,
    ) -> CalculationControlRequest {
        CalculationControlRequest {
            scope: "one".into(),
            model_id: "profile-test".into(),
            expected_history: history,
            request,
        }
    }
    fn start(history: SessionHistoryState, id: &str, analyze: bool) -> CalculationControlRequest {
        control(
            Some(history),
            CalculationArguments::Start {
                job_id: id.into(),
                display_case: Some(DisplayCase::SagReference),
                analyze,
                scene_transfer: Default::default(),
            },
        )
    }
    fn status(runtime: &mut Runtime, id: &str) -> Value {
        runtime
            .execute(control(
                None,
                CalculationArguments::Status { job_id: id.into() },
            ))
            .unwrap()
    }
    fn ready(runtime: &mut Runtime, id: &str) {
        let until = Instant::now() + Duration::from_secs(10);
        loop {
            let value = status(runtime, id);
            assert!(serde_json::to_vec(&value).unwrap().len() < 2048);
            if value["status"] != "running" {
                assert_eq!(value["status"], "ready");
                break;
            }
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    #[test]
    fn public_job_arguments_close_transport_scope_history_and_publication_inputs() {
        for raw in [
            r#"{"action":"start","job_id":"a","scope":"other"}"#,
            r#"{"action":"start","job_id":"a","publication_id":"forged"}"#,
            r#"{"action":"start","job_id":"a","analyze":"yes"}"#,
            r#"{"action":"status","job_id":"a","analyze":true}"#,
            r#"{"action":"save","job_id":"a"}"#,
        ] {
            assert!(
                serde_json::from_str::<CalculationArguments>(raw).is_err(),
                "{raw}"
            );
        }
        for id in ["", "\n", &"x".repeat(129)] {
            assert!(CalculationArguments::Cancel { job_id: id.into() }
                .validate()
                .is_err());
        }
        let args: CalculationArguments =
            serde_json::from_str(r#"{"action":"start","job_id":"a"}"#).unwrap();
        assert!(matches!(
            args,
            CalculationArguments::Start {
                analyze: false,
                display_case: None,
                ..
            }
        ));
        let wire = json!({"operation":"calculation_control", "scope":"one", "model_id":"profile-test", "request":{"action":"status","job_id":"a"}});
        assert!(matches!(
            serde_json::from_value::<Request>(wire.clone()).unwrap(),
            Request::CalculationControl(_)
        ));
        let mut forged = wire;
        forged["publication_id"] = json!("caller-pin");
        assert!(serde_json::from_value::<Request>(forged).is_err());
    }

    #[test]
    fn public_start_requires_exact_held_history_and_never_changes_results_on_refusal() {
        let mut runtime = Runtime::default();
        let input = input();
        let initial = runtime.execute(input.clone()).unwrap();
        let history = runtime.held["one"].session.history_state();
        let mut missing = start(history.clone(), "missing-history", false);
        missing.expected_history = None;
        assert!(runtime
            .execute(missing)
            .unwrap_err()
            .starts_with("profile_replay_stale:"));
        let mut different = start(history.clone(), "different-model", false);
        different.model_id = "other".into();
        assert!(runtime
            .execute(different)
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        let mut future = history.clone();
        future.undo_depth += 1;
        assert!(runtime
            .execute(start(future, "different-history", false))
            .unwrap_err()
            .starts_with("profile_replay_stale:"));
        assert!(runtime
            .execute(control(
                None,
                CalculationArguments::Status {
                    job_id: "missing-history".into()
                }
            ))
            .unwrap_err()
            .starts_with("profile_calculation_unavailable:"));
        assert_eq!(runtime.held["one"].session.history_state(), history);
        assert_eq!(
            runtime.held["one"]
                .review
                .base_scene()
                .unwrap()
                .model_revision
                .as_str(),
            initial["model_revision"].as_str().unwrap()
        );
    }

    #[test]
    fn public_native_jobs_capture_inputs_and_admit_only_through_publication_with_fresh_parity() {
        let input = input();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let history = runtime.held["one"].session.history_state();
        let sequence = runtime.held["one"].session.clone().checkpoint().sequence;
        let ticket = runtime
            .execute(start(history.clone(), "public-parity", true))
            .unwrap();
        assert_eq!(ticket["history"], json!(history));
        assert!(ticket["scene"].is_null());
        assert!(serde_json::to_vec(&ticket).unwrap().len() < 2048);
        ready(&mut runtime, "public-parity");
        let result = runtime
            .execute(control(
                Some(history.clone()),
                CalculationArguments::Admit {
                    job_id: "public-parity".into(),
                },
            ))
            .unwrap();
        let mut expected = input.clone();
        expected.analyze = true;
        expected.patch.display_case = Some(DisplayCase::SagReference);
        let fresh = Runtime::default().execute(expected).unwrap();
        assert_eq!(result["scene"], fresh["scene"]);
        assert_eq!(result["analysis"], fresh["analysis"]);
        let pin: PublicationPin = serde_json::from_value(result["publication"].clone()).unwrap();
        assert_eq!(pin.publication_id, "public-parity");
        assert_eq!(
            runtime.held["one"].session.clone().checkpoint().sequence,
            sequence
        );
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
            runtime.views["one"].display_case,
            DisplayState::default().display_case
        );
        assert!(runtime
            .execute(start(history, "public-parity", false))
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
    }

    #[test]
    fn public_job_status_and_cancel_are_isolated_and_edits_continue_while_worker_runs() {
        let input = input();
        let mut runtime = Runtime::default();
        runtime.execute(input.clone()).unwrap();
        let history = runtime.held["one"].session.history_state();
        let mut worker = runtime.held["one"].session.clone();
        let (release, entered) = runtime.calculation_jobs.pause_next();
        runtime
            .execute(start(history.clone(), "public-edit", false))
            .unwrap();
        entered.recv_timeout(Duration::from_secs(2)).unwrap();
        let mut another = control(
            None,
            CalculationArguments::Cancel {
                job_id: "public-edit".into(),
            },
        );
        another.model_id = "other".into();
        assert!(runtime
            .execute(another)
            .unwrap_err()
            .starts_with("profile_replay_model_mismatch:"));
        assert_eq!(status(&mut runtime, "public-edit")["status"], "running");
        describe(&mut worker, "public-local-edit", "live authored edit");
        let observed = runtime
            .execute(Request::Observe(Box::new(refresh(
                &runtime,
                &input,
                &history.model_revision.to_string(),
                &worker,
            ))))
            .unwrap();
        assert_eq!(observed["ran"], false);
        assert_eq!(
            runtime.held["one"].session.history_state(),
            worker.history_state()
        );
        assert_eq!(status(&mut runtime, "public-edit")["status"], "stale");
        assert!(runtime
            .execute(control(
                None,
                CalculationArguments::Admit {
                    job_id: "public-edit".into()
                }
            ))
            .unwrap_err()
            .starts_with("profile_calculation_stale:"));
        let cancelled = runtime
            .execute(control(
                None,
                CalculationArguments::Cancel {
                    job_id: "public-edit".into(),
                },
            ))
            .unwrap();
        assert_eq!(cancelled["runtime_worker_active"], true);
        assert!(runtime
            .execute(start(worker.history_state(), "public-busy", false))
            .unwrap_err()
            .starts_with("profile_calculation_busy:"));
        release.send(()).unwrap();
        let until = Instant::now() + Duration::from_secs(2);
        while status(&mut runtime, "public-edit")["runtime_worker_active"] == true {
            assert!(Instant::now() < until);
            std::thread::sleep(Duration::from_millis(2));
        }
    }
}

impl CalculationArguments {
    pub fn validate(&self) -> Result<(), String> {
        if let Self::PrepareSceneBase {} = self {
            return Ok(());
        }
        if let Self::Start {
            scene_transfer: CalculationTransfer::Delta { base_signature },
            ..
        } = self
        {
            if base_signature.len() != 71
                || !base_signature.starts_with("sha256:")
                || !base_signature[7..]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit())
            {
                return Err("invalid_profile_view: invalid calculated scene base signature".into());
            }
        }
        let job_id = match self {
            Self::Start { job_id, .. }
            | Self::Status { job_id }
            | Self::Cancel { job_id }
            | Self::Admit { job_id } => job_id,
            Self::PrepareSceneBase {} => unreachable!(),
        };
        calculation_jobs::identity(job_id)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CalculationControlRequest {
    pub scope: String,
    pub model_id: String,
    #[serde(default)]
    pub expected_history: Option<SessionHistoryState>,
    pub request: CalculationArguments,
}

impl From<CalculationControlRequest> for Request {
    fn from(input: CalculationControlRequest) -> Self {
        Self::CalculationControl(input)
    }
}

impl Runtime {
    pub(super) fn prepare_calculation_control(
        &self,
        input: CalculationControlRequest,
    ) -> Result<Request, String> {
        input.request.validate()?;
        if input.scope.is_empty()
            || input.scope.len() > 1024
            || input.model_id.is_empty()
            || input.model_id.len() > 200
        {
            return Err("invalid_profile_view: invalid calculation scope or model".into());
        }
        let held = self
            .held
            .get(&input.scope)
            .ok_or("profile_replay_base_unavailable: initialize this model scope first")?;
        if held.source.model_id != input.model_id {
            return Err(
                "profile_replay_model_mismatch: calculation belongs to another model".into(),
            );
        }
        let history = held.session.history_state();
        if input
            .expected_history
            .as_ref()
            .is_some_and(|expected| expected != &history)
        {
            return Err(
                "profile_replay_stale: calculation control names another authored history".into(),
            );
        }
        let reference = |job_id| CalculationJobRequest {
            scope: input.scope.clone(),
            model_id: input.model_id.clone(),
            job_id,
        };
        Ok(match input.request {
            CalculationArguments::Start {
                job_id,
                display_case,
                analyze,
                scene_transfer,
            } => {
                if input.expected_history.is_none() {
                    return Err(
                        "profile_replay_stale: start requires the exact observed authored history"
                            .into(),
                    );
                }
                // The single-use native job identity also owns its publication.
                // Admission retrieves this pin from the native job, not the host.
                let publication_id = job_id.clone();
                let patch = DisplayPatch {
                    display_case,
                    ..Default::default()
                };
                let journal_json = serde_json::to_string(&held.session.retained_journal())
                    .map_err(|error| format!("profile_replay_invalid: {error}"))?;
                let start = Box::new(StartCalculationRequest {
                    job_id,
                    request: RefreshRequest {
                        scope: input.scope,
                        model_id: input.model_id,
                        publication_id: Some(publication_id),
                        base_revision: history.model_revision.clone(),
                        base_history: history.clone(),
                        expected_revision: history.model_revision.clone(),
                        target_history: history,
                        journal_json,
                        options: held.options.clone(),
                        style_document: held.style_document.as_ref().clone(),
                        patch,
                        analyze,
                        calculated_base_signature: None,
                    },
                });
                match scene_transfer {
                    CalculationTransfer::Full {} => Request::StartCalculation(start),
                    CalculationTransfer::ExactJson {} => Request::StartExactCalculation(start),
                    CalculationTransfer::Delta {base_signature} => Request::StartDeltaCalculation(Box::new(StartDeltaCalculationRequest {
                        base_signature,calculation:*start,
                    })),
                }
            }
            CalculationArguments::PrepareSceneBase {} => Request::CalculatedSceneBase(CalculatedSceneBaseRequest {
                scope:input.scope,model_id:input.model_id,
                expected_history:input.expected_history.ok_or("profile_replay_stale: scene-base preparation requires exact observed history")?,
            }),
            CalculationArguments::Status { job_id } => {
                Request::CalculationStatus(reference(job_id))
            }
            CalculationArguments::Cancel { job_id } => {
                Request::CancelCalculation(reference(job_id))
            }
            CalculationArguments::Admit { job_id } => {
                let publication_id = self.calculation_publication(&reference(job_id.clone()))?;
                Request::AdmitCalculation(AdmitCalculationRequest {
                    scope: input.scope,
                    model_id: input.model_id,
                    job_id,
                    publication_id: Some(publication_id),
                })
            }
        })
    }
}
