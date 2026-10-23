//! Deterministic portfolio batch command. Hosts execute effects, persist state,
//! and supply completion events; no scheduler or cancellation policy lives there.
use crate::is_solar_id as id;
use crate::module_engine::schema_family;
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
        // The request stays closed; its revision is provenance (contract 4).
        if schema_family(&self.schema) != schema_family(SCHEMA)
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
#[path = "solar_batch_tests.rs"]
mod tests;
