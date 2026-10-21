//! Deterministic portfolio batch command. Hosts execute effects, persist state,
//! and supply completion events; no scheduler or cancellation policy lives there.
use crate::is_solar_id as id;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

pub const SCHEMA: &str = "ds-solar.portfolio-batch/v1";
pub const MAX_BYTES: usize = 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Portfolio {
    pub id: String,
    pub name: String,
    pub cities: Vec<String>,
    pub membership_revision: String,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Placement {
    Native,
    Cloud,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub schema: String,
    pub batch_id: String,
    pub root: String,
    pub portfolios: Vec<Portfolio>,
    pub parallel: usize,
    pub graph_strategy: String,
    pub force: bool,
    pub render_charts: bool,
    pub report_drafts: Vec<String>,
    pub language: String,
    pub placement: Placement,
}
impl Request {
    pub fn validate_catalog(&self, catalog: &[Portfolio]) -> Result<(), String> {
        self.validate()?;
        for selected in &self.portfolios {
            if !catalog.iter().any(|p| {
                p.id == selected.id
                    && p.name == selected.name
                    && p.membership_revision == selected.membership_revision
                    && p.cities == selected.cities
            }) {
                return Err(format!(
                    "portfolio {} changed or is absent from the project catalog; list again",
                    selected.id
                ));
            }
        }
        Ok(())
    }
    pub fn validate(&self) -> Result<(), String> {
        if self.schema != SCHEMA
            || !id(&self.batch_id)
            || self.batch_id.len() > 80
            || self.root.is_empty()
            || self.root.len() > 256
            || self.root.trim() != self.root
        {
            return Err("invalid portfolio batch schema, id or project root".into());
        }
        if self.portfolios.is_empty()
            || self.portfolios.len() > 100
            || !(1..=16).contains(&self.parallel)
        {
            return Err("select 1..100 portfolios and 1..16 concurrent portfolios".into());
        }
        let bytes = serde_json::to_vec(&self.portfolios).map_err(|e| e.to_string())?;
        crate::validate_solar_portfolio_catalog(&bytes).map_err(|e| e.to_string())?;
        if !["fr", "en"].contains(&self.language.as_str())
            || self.report_drafts.is_empty()
            || self.report_drafts.len() > 4
            || self
                .report_drafts
                .iter()
                .any(|draft| !["apd", "network", "plant", "financial"].contains(&draft.as_str()))
            || self.report_drafts.iter().collect::<BTreeSet<_>>().len() != self.report_drafts.len()
        {
            return Err("invalid portfolio language or report drafts".into());
        }
        // The deployed hosted endpoint has no source-refresh or draft-option
        // contract. Refuse overrides rather than silently ignoring them.
        if self.placement == Placement::Cloud
            && (self.force
                || !self.render_charts
                || self.language != "fr"
                || self.report_drafts != ["apd"])
        {
            return Err("hosted portfolios support server defaults only; source refresh and custom draft options require native placement".into());
        }
        if !["first", "round_robin"].contains(&self.graph_strategy.as_str())
            && !self
                .graph_strategy
                .strip_prefix("city:")
                .is_some_and(|city| {
                    self.portfolios
                        .iter()
                        .all(|p| p.cities.iter().any(|c| c == city))
                })
        {
            return Err("graph strategy must apply to every selected portfolio".into());
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Source {
    pub run_id: String,
    pub city_ids: Vec<String>,
    pub status: String,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct NativeGraphSelection {
    pub representative_strategy: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub representative_city_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub round_robin_start_index: Option<u64>,
}

fn native_graph_selection(strategy: &str) -> NativeGraphSelection {
    let city = strategy.strip_prefix("city:");
    NativeGraphSelection {
        representative_strategy: if city.is_some() { "explicit" } else { strategy }.into(),
        representative_city_id: city.map(str::to_owned),
        round_robin_start_index: (strategy == "round_robin").then_some(0),
    }
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Phase {
    Queued,
    Cities,
    Portfolio,
    Ready,
    Failed,
    Cancelled,
}
impl Phase {
    fn active(self) -> bool {
        matches!(self, Self::Cities | Self::Portfolio)
    }
    fn terminal(self) -> bool {
        matches!(self, Self::Ready | Self::Failed | Self::Cancelled)
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Row {
    pub phase: Phase,
    pub source_run_id: Option<String>,
    pub run_id: String,
    pub started_at: Option<u64>,
    pub completed_at: Option<u64>,
    pub detail: Option<String>,
    pub cancel_acknowledged: bool,
    pub cancel_pending: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct State {
    pub request: Request,
    pub rows: Vec<Row>,
    pub cancelled: bool,
    pub complete: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Event {
    SourceCompleted {
        index: usize,
        run_id: String,
        status: String,
        detail: Option<String>,
    },
    SourceSucceeded {
        index: usize,
        run_id: String,
    },
    Succeeded {
        index: usize,
        run_id: String,
        detail: Option<String>,
    },
    Failed {
        index: usize,
        run_id: String,
        detail: String,
    },
    CancelAcknowledged {
        index: usize,
        run_id: String,
        accepted: bool,
    },
    Cancel,
    Tick,
    Recover,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Effect {
    Source {
        index: usize,
        run_id: String,
    },
    Portfolio {
        index: usize,
        run_id: String,
        selection: NativeGraphSelection,
    },
    Cloud {
        index: usize,
        run_id: String,
        graph_selection: String,
    },
    Cancel {
        index: usize,
        run_id: String,
    },
    Wake {
        after_ms: u64,
    },
}
#[derive(Debug, Serialize, Deserialize)]
pub struct Output {
    pub state: State,
    pub effects: Vec<Effect>,
}
#[derive(Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
enum Command {
    Start {
        request: Request,
        sources: Vec<Source>,
        catalog: Vec<Portfolio>,
        now: u64,
    },
    Event {
        state: State,
        event: Event,
        now: u64,
    },
}

pub fn evaluate(bytes: &[u8]) -> Result<String, String> {
    if bytes.is_empty() || bytes.len() > MAX_BYTES {
        return Err("portfolio batch exceeds 1 MiB".into());
    }
    let command: Command = serde_json::from_slice(bytes).map_err(|e| e.to_string())?;
    let output = match command {
        Command::Start {
            request,
            sources,
            catalog,
            now,
        } => {
            request.validate_catalog(&catalog)?;
            start(request, &sources, now)?
        }
        Command::Event { state, event, now } => transition(state, event, now)?,
    };
    serde_json::to_string(&output).map_err(|e| e.to_string())
}
pub fn start(request: Request, sources: &[Source], now: u64) -> Result<Output, String> {
    request.validate()?;
    if sources.len() > 100 {
        return Err("portfolio source catalog exceeds 100 runs".into());
    }
    let rows = request
        .portfolios
        .iter()
        .enumerate()
        .map(|(index, p)| Row {
            phase: Phase::Queued,
            source_run_id: if request.force {
                None
            } else {
                sources
                    .iter()
                    .find(|source| {
                        source.status == "succeeded"
                            && id(&source.run_id)
                            && source.city_ids == p.cities
                    })
                    .map(|s| s.run_id.clone())
            },
            run_id: format!("{}-{index}-source", request.batch_id),
            started_at: None,
            completed_at: None,
            detail: None,
            cancel_acknowledged: false,
            cancel_pending: false,
        })
        .collect();
    let state = State {
        request,
        rows,
        cancelled: false,
        complete: false,
    };
    transition(state, Event::Tick, now)
}
fn active_row<'a>(state: &'a mut State, index: usize, run_id: &str) -> Result<&'a mut Row, String> {
    let row = state
        .rows
        .get_mut(index)
        .ok_or("unknown portfolio event index")?;
    if row.run_id != run_id {
        return Err("portfolio event belongs to another execution".into());
    }
    Ok(row)
}
fn finish(row: &mut Row, phase: Phase, now: u64, detail: Option<String>) {
    row.phase = phase;
    row.completed_at = Some(now);
    row.detail = detail.map(|value| value.chars().take(2000).collect());
}
pub fn transition(mut state: State, event: Event, now: u64) -> Result<Output, String> {
    state.request.validate()?;
    if state.rows.len() != state.request.portfolios.len() {
        return Err("portfolio state membership differs from request".into());
    }
    let retry_cancel = matches!(event, Event::Cancel | Event::Tick);
    let cancelled = state.cancelled;
    match event {
        Event::SourceCompleted {
            index,
            run_id,
            status,
            detail,
        } => {
            return transition(
                state,
                if status == "succeeded" {
                    Event::SourceSucceeded { index, run_id }
                } else {
                    Event::Failed {
                        index,
                        run_id,
                        detail: detail.unwrap_or_else(|| "Source city batch failed.".into()),
                    }
                },
                now,
            );
        }
        Event::SourceSucceeded { index, run_id } => {
            let row = active_row(&mut state, index, &run_id)?;
            if row.phase == Phase::Cities {
                if cancelled {
                    finish(row, Phase::Cancelled, now, None);
                } else {
                    row.source_run_id = Some(run_id);
                    row.phase = Phase::Queued;
                }
            }
        }
        Event::Succeeded {
            index,
            run_id,
            detail,
        } => {
            let row = active_row(&mut state, index, &run_id)?;
            // A committed result wins a cancellation race.
            if row.phase == Phase::Portfolio {
                finish(row, Phase::Ready, now, detail);
            }
        }
        Event::Failed {
            index,
            run_id,
            detail,
        } => {
            let row = active_row(&mut state, index, &run_id)?;
            if row.phase.active() {
                finish(
                    row,
                    if cancelled {
                        Phase::Cancelled
                    } else {
                        Phase::Failed
                    },
                    now,
                    Some(detail),
                );
            }
        }
        Event::CancelAcknowledged {
            index,
            run_id,
            accepted,
        } => {
            let row = active_row(&mut state, index, &run_id)?;
            row.cancel_pending = false;
            if row.phase.active() && accepted {
                row.cancel_acknowledged = true;
            }
        }
        Event::Cancel => state.cancelled = true,
        Event::Recover => {
            // No implicit replay after a host restart. Already sealed individual
            // runs remain addressable, while interrupted work needs a new command.
            for row in &mut state.rows {
                if !row.phase.terminal() {
                    finish(row, Phase::Failed, now, Some("Interrupted before batch completion; inspect sealed results before rerunning.".into()));
                }
            }
        }
        Event::Tick => {}
    }
    let mut effects = Vec::new();
    let mut active = state.rows.iter().filter(|r| r.phase.active()).count();
    for (index, row) in state.rows.iter_mut().enumerate() {
        if row.phase == Phase::Queued {
            if state.cancelled {
                finish(row, Phase::Cancelled, now, None);
                continue;
            }
            if active >= state.request.parallel {
                continue;
            }
            active += 1;
            row.started_at.get_or_insert(now);
            if state.request.placement == Placement::Cloud || row.source_run_id.is_some() {
                row.phase = Phase::Portfolio;
                row.run_id = format!("{}-{index}-portfolio", state.request.batch_id);
                effects.push(if state.request.placement == Placement::Cloud {
                    Effect::Cloud {
                        index,
                        run_id: row.run_id.clone(),
                        graph_selection: state
                            .request
                            .graph_strategy
                            .strip_prefix("city:")
                            .unwrap_or(&state.request.graph_strategy)
                            .into(),
                    }
                } else {
                    Effect::Portfolio {
                        index,
                        run_id: row.run_id.clone(),
                        selection: native_graph_selection(&state.request.graph_strategy),
                    }
                });
            } else {
                row.phase = Phase::Cities;
                effects.push(Effect::Source {
                    index,
                    run_id: row.run_id.clone(),
                });
            }
        }
        if retry_cancel
            && state.cancelled
            && row.phase.active()
            && !row.cancel_acknowledged
            && !row.cancel_pending
            && state.request.placement == Placement::Native
        {
            row.cancel_pending = true;
            effects.push(Effect::Cancel {
                index,
                run_id: row.run_id.clone(),
            });
        }
    }
    if state.cancelled
        && state.request.placement == Placement::Native
        && state
            .rows
            .iter()
            .any(|r| r.phase.active() && !r.cancel_acknowledged)
    {
        effects.push(Effect::Wake { after_ms: 100 });
    }
    state.complete = state.rows.iter().all(|row| row.phase.terminal());
    Ok(Output { state, effects })
}

#[cfg(test)]
mod tests {
    use super::*;
    fn request() -> Request {
        Request {
            schema: SCHEMA.into(),
            batch_id: "batch-test".into(),
            root: "root".into(),
            portfolios: (0..3)
                .map(|i| Portfolio {
                    id: format!("p{i}"),
                    name: format!("P{i}"),
                    cities: vec!["a".into(), "b".into()],
                    membership_revision: format!("sha256:{}", "a".repeat(64)),
                })
                .collect(),
            parallel: 2,
            graph_strategy: "round_robin".into(),
            force: false,
            render_charts: true,
            report_drafts: vec!["apd".into()],
            language: "fr".into(),
            placement: Placement::Native,
        }
    }
    fn sources() -> Vec<Source> {
        vec![Source {
            run_id: "source".into(),
            city_ids: vec!["a".into(), "b".into()],
            status: "succeeded".into(),
        }]
    }
    #[test]
    fn batch_rejects_ids_outside_the_contract_grammar() {
        for candidate in ["-x", "_x", "a.b", &"a".repeat(129)] {
            let mut request = request();
            request.portfolios[0].id = candidate.into();
            assert!(request.validate().is_err(), "{candidate}");
        }
    }

    #[test]
    fn bounded_parallelism_and_failure_isolation() {
        let first = start(request(), &sources(), 1).unwrap();
        assert_eq!(first.effects.len(), 2);
        assert_eq!(first.state.rows[2].phase, Phase::Queued);
        let next = transition(
            first.state,
            Event::Failed {
                index: 0,
                run_id: "batch-test-0-portfolio".into(),
                detail: "bad input".into(),
            },
            2,
        )
        .unwrap();
        assert_eq!(next.effects.len(), 1);
        assert_eq!(next.state.rows[0].phase, Phase::Failed);
        assert_eq!(next.state.rows[2].phase, Phase::Portfolio);
    }

    #[test]
    fn effects_carry_owner_resolved_graph_selection_and_failed_sources_never_aggregate() {
        for (strategy, expected, city, offset) in [
            ("first", "first", None, None),
            ("round_robin", "round_robin", None, Some(0)),
            ("city:b", "explicit", Some("b"), None),
        ] {
            let mut request = request();
            request.graph_strategy = strategy.into();
            let output = start(request, &sources(), 1).unwrap();
            match &output.effects[0] {
                Effect::Portfolio { selection, .. } => {
                    assert_eq!(selection.representative_strategy, expected);
                    assert_eq!(selection.representative_city_id.as_deref(), city);
                    assert_eq!(selection.round_robin_start_index, offset);
                }
                _ => panic!("expected portfolio effect"),
            }
        }
        let mut request = request();
        request.portfolios.truncate(1);
        let output = start(request, &[], 1).unwrap();
        let output = transition(
            output.state,
            Event::SourceCompleted {
                index: 0,
                run_id: "batch-test-0-source".into(),
                status: "failed".into(),
                detail: Some("missing city".into()),
            },
            2,
        )
        .unwrap();
        assert!(output.effects.is_empty());
        assert_eq!(output.state.rows[0].phase, Phase::Failed);
        assert_eq!(output.state.rows[0].detail.as_deref(), Some("missing city"));
    }
    #[test]
    fn source_order_force_and_governed_revision_are_exact() {
        let mut source = sources();
        source[0].city_ids.reverse();
        assert_eq!(
            start(request(), &source, 1).unwrap().state.rows[0].phase,
            Phase::Cities
        );
        let mut force = request();
        force.force = true;
        assert_eq!(
            start(force, &sources(), 1).unwrap().state.rows[0].phase,
            Phase::Cities
        );
        let mut changed = request().portfolios;
        changed[0].cities.reverse();
        assert!(request().validate_catalog(&changed).is_err());
        assert!(request().validate_catalog(&request().portfolios).is_ok());
    }
    // Migrated from ds-web/solar/portfolio-source-run.test.ts. Source selection
    // has one implementation and test authority for native CLI and WASM hosts.
    #[test]
    fn source_selection_rejects_ineligible_runs_and_uses_newest_exact_membership() {
        let mut candidates = vec![
            Source {
                run_id: "running".into(),
                city_ids: vec!["a".into(), "b".into()],
                status: "running".into(),
            },
            Source {
                run_id: "partial".into(),
                city_ids: vec!["a".into()],
                status: "succeeded".into(),
            },
            Source {
                run_id: "reordered".into(),
                city_ids: vec!["b".into(), "a".into()],
                status: "succeeded".into(),
            },
            Source {
                run_id: " padded ".into(),
                city_ids: vec!["a".into(), "b".into()],
                status: "succeeded".into(),
            },
        ];
        // Each negative control must be ineligible on its own too.
        for candidate in &candidates {
            let output = start(request(), std::slice::from_ref(candidate), 1).unwrap();
            assert!(output
                .state
                .rows
                .iter()
                .all(|row| row.source_run_id.is_none()));
            assert!(output
                .effects
                .iter()
                .all(|effect| matches!(effect, Effect::Source { .. })));
        }
        let output = start(request(), &candidates, 1).unwrap();
        assert!(output
            .state
            .rows
            .iter()
            .all(|row| row.source_run_id.is_none()));
        for run_id in ["newest-exact", "older-exact"] {
            candidates.push(Source {
                run_id: run_id.into(),
                city_ids: vec!["a".into(), "b".into()],
                status: "succeeded".into(),
            });
        }
        let output = start(request(), &candidates, 1).unwrap();
        assert!(output
            .state
            .rows
            .iter()
            .all(|row| row.source_run_id.as_deref() == Some("newest-exact")));
        assert_eq!(output.effects.len(), 2);
        assert!(output
            .effects
            .iter()
            .all(|effect| matches!(effect, Effect::Portfolio { .. })));
        assert_eq!(output.state.rows[2].phase, Phase::Queued);
    }

    #[test]
    fn graph_city_must_belong_to_every_selected_portfolio() {
        let mut req = request();
        req.graph_strategy = "city:a".into();
        assert!(req.validate().is_ok());
        req.portfolios[1].cities = vec!["b".into(), "c".into()];
        assert_eq!(
            req.validate().unwrap_err(),
            "graph strategy must apply to every selected portfolio"
        );
        req.graph_strategy = "city:b".into();
        assert!(req.validate().is_ok());
        req.graph_strategy = "made_up".into();
        assert!(req.validate().is_err());
    }
    #[test]
    fn cancellation_waits_for_registration_without_spinning_and_commit_wins() {
        let started = start(request(), &sources(), 1).unwrap();
        let cancelled = transition(started.state, Event::Cancel, 2).unwrap();
        assert_eq!(cancelled.state.rows[2].phase, Phase::Cancelled);
        assert_eq!(cancelled.effects.len(), 3);
        let unregistered = transition(
            cancelled.state,
            Event::CancelAcknowledged {
                index: 0,
                run_id: "batch-test-0-portfolio".into(),
                accepted: false,
            },
            3,
        )
        .unwrap();
        assert!(matches!(
            unregistered.effects.as_slice(),
            [Effect::Wake { .. }]
        ));
        let retry = transition(unregistered.state, Event::Tick, 4).unwrap();
        assert_eq!(retry.effects.len(), 2);
        let committed = transition(
            retry.state,
            Event::Succeeded {
                index: 0,
                run_id: "batch-test-0-portfolio".into(),
                detail: None,
            },
            5,
        )
        .unwrap();
        assert_eq!(committed.state.rows[0].phase, Phase::Ready);
        let stopped = transition(
            committed.state,
            Event::Failed {
                index: 1,
                run_id: "batch-test-1-portfolio".into(),
                detail: "cancelled".into(),
            },
            6,
        )
        .unwrap();
        assert!(stopped.state.complete);
        assert!(stopped.effects.is_empty());
    }
    #[test]
    fn cancelled_source_never_starts_aggregation_and_restart_never_replays() {
        let first = start(request(), &[], 1).unwrap();
        let cancelled = transition(first.state, Event::Cancel, 2).unwrap();
        let source_done = transition(
            cancelled.state,
            Event::SourceSucceeded {
                index: 0,
                run_id: "batch-test-0-source".into(),
            },
            3,
        )
        .unwrap();
        assert_eq!(source_done.state.rows[0].phase, Phase::Cancelled);
        assert!(source_done
            .effects
            .iter()
            .all(|e| matches!(e, Effect::Wake { .. })));
        let recovered = transition(source_done.state, Event::Recover, 4).unwrap();
        assert!(recovered.state.complete);
        assert!(recovered.effects.is_empty());
    }
    #[test]
    fn foreign_events_unknown_fields_and_invalid_limits_are_refused() {
        let first = start(request(), &[], 1).unwrap();
        assert!(transition(
            first.state,
            Event::SourceSucceeded {
                index: 0,
                run_id: "other".into()
            },
            2
        )
        .is_err());
        let mut r = request();
        r.parallel = 0;
        assert!(r.validate().is_err());
        let mut value = serde_json::to_value(request()).unwrap();
        value["fallback"] = true.into();
        assert!(serde_json::from_value::<Request>(value).is_err());
    }
    #[test]
    fn shared_wire_matches_native_transition() {
        let r = request();
        let wire = serde_json::json!({"operation":"start", "request":r, "catalog":r.portfolios, "sources":sources(), "now":1});
        let actual: serde_json::Value =
            serde_json::from_str(&evaluate(&serde_json::to_vec(&wire).unwrap()).unwrap()).unwrap();
        assert_eq!(
            actual,
            serde_json::to_value(start(request(), &sources(), 1).unwrap()).unwrap()
        );
    }
}
