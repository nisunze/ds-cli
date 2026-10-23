//! The one Kernel owner of every Core → Solar addon call.
//!
//! A Solar task is a typed request the host wrote and a closed result the
//! addon writes, or the addon's own refusal. This client captures exactly one
//! lane's executable, reads that executable's identity and task catalogue
//! (`build-info` `.schemas`) at most once, and refuses a task the installed
//! addon does not declare before any work starts. Hosts never name a
//! subcommand, build an argument vector or start the executable: they choose
//! a closed [`Task`] and hand over their request and output paths, or run the
//! host's bounded task transport inside [`Solar::with_task_executable`].
//!
//! Compatibility is the catalogue's answer for the task actually run, never a
//! version, build date or digest equality with this Core.
use crate::{CapturedExternal, Completed, addons::SOLAR};
use ds_core_contracts::Failure;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// Discovery answers immediately or something is wrong.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
/// The stable code for a task the installed addon does not declare.
pub const TASK_UNSUPPORTED: &str = "unknown_task";
const ENGINE_NAME: &str = "ds-solar-engine";

/// Every Solar task Core calls.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    /// One complete prepared city into a fresh artifact directory.
    CalculatePrepared,
    /// An ordered prepared-city batch into a fresh artifact directory.
    CalculateBatch,
    /// One exact saved city batch aggregated into a portfolio.
    CalculatePortfolio,
}

impl Task {
    pub const fn name(self) -> &'static str {
        match self {
            Self::CalculatePrepared => "calculate_prepared",
            Self::CalculateBatch => "calculate_batch",
            Self::CalculatePortfolio => "calculate_portfolio",
        }
    }

    /// The fixed process subcommand; only this owner passes it to a process.
    pub const fn subcommand(self) -> &'static str {
        match self {
            Self::CalculatePrepared => "calculate-prepared",
            Self::CalculateBatch => "calculate-batch",
            Self::CalculatePortfolio => "calculate-portfolio",
        }
    }

    /// The typed task document the addon's catalogue must declare for it.
    pub const fn catalogue_schema(self) -> &'static str {
        match self {
            Self::CalculatePrepared => ds_solar_contracts::PREPARED_CALCULATION_TASK_SCHEMA,
            Self::CalculateBatch => {
                ds_solar_contracts::prepared_batch::PREPARED_BATCH_REQUEST_SCHEMA
            }
            Self::CalculatePortfolio => {
                ds_solar_contracts::portfolio_task::PORTFOLIO_TASK_REQUEST_SCHEMA
            }
        }
    }
}

/// One accepted run's Solar addon: the captured executable of one lane, its
/// identity read at most once, and every task invoked through it.
pub struct Solar {
    engine: CapturedExternal,
    identity: OnceLock<Value>,
}

/// The addon's answer to one file task: its exit and bounded streams. The
/// result stays at the host's output path, read against its typed receipt.
pub struct Reply {
    task: Task,
    completed: Completed,
}

impl Solar {
    /// Capture the explicit installed lane (or the deliberate development override).
    pub fn capture_for_lane(lane: &str) -> Result<Self, Failure> {
        SOLAR.capture_for_lane(lane).map(Self::from_captured)
    }

    /// Native-owned relocated addon store, never a command-supplied path.
    pub fn capture_for_lane_in(lane: &str, base: &Path) -> Result<Self, Failure> {
        SOLAR.capture_for_lane_in(lane, base).map(Self::from_captured)
    }

    /// Reopen one exact executable a retained run or a test fixture pinned.
    pub fn capture_pinned(path: &Path) -> Result<Self, Failure> {
        SOLAR.capture_pinned(path).map(Self::from_captured)
    }

    fn from_captured(engine: CapturedExternal) -> Self {
        Self {
            engine,
            identity: OnceLock::new(),
        }
    }

    /// The canonical executable this run captured; never resolved again.
    pub fn executable(&self) -> &Path {
        self.engine.executable()
    }

    /// The captured addon's own identity (`build-info`), read once. Its
    /// `schemas` list is the task catalogue.
    pub fn build_info(&self) -> Result<&Value, Failure> {
        if let Some(identity) = self.identity.get() {
            return Ok(identity);
        }
        let read = self
            .engine
            .call_json("build-info", &[], DISCOVERY_TIMEOUT)?;
        if read["name"] != ENGINE_NAME || !read["schemas"].is_array() {
            return Err(Failure::failed(
                "callee_contract_mismatch",
                "`solar build-info` returned no Solar engine identity and task catalogue",
            )
            .remedy("install compatible ds and Solar addon versions"));
        }
        Ok(self.identity.get_or_init(|| read))
    }

    /// Refuse, before any work, a task this addon does not declare.
    pub fn require(&self, task: Task) -> Result<&Value, Failure> {
        let identity = self.build_info()?;
        let declared = identity["schemas"]
            .as_array()
            .into_iter()
            .flatten()
            .any(|schema| schema == task.catalogue_schema());
        if declared {
            Ok(identity)
        } else {
            Err(unsupported(task))
        }
    }

    /// Run one declared file task: `--request <file> --out <fresh-dir>`. The
    /// catalogue check precedes the process; the receipt check of a managed
    /// executable brackets it.
    pub fn run(
        &self,
        task: Task,
        request: &Path,
        out: &Path,
        timeout: Duration,
    ) -> Result<Reply, Failure> {
        self.require(task)?;
        let args: [OsString; 4] = [
            "--request".into(),
            request.into(),
            "--out".into(),
            out.into(),
        ];
        let completed = self.engine.call(task.subcommand(), &args, timeout)?;
        Ok(Reply { task, completed })
    }

    /// Run the host's bounded task transport for one declared task on the
    /// captured executable. The catalogue check comes first; the managed
    /// receipt is checked before and after, including when the transport fails.
    pub fn with_task_executable<T>(
        &self,
        task: Task,
        transport: impl FnOnce(&Path) -> T,
    ) -> Result<T, Failure> {
        self.require(task)?;
        self.engine.with_verified_executable(transport)
    }
}

impl Reply {
    pub fn task(&self) -> Task {
        self.task
    }
    pub fn succeeded(&self) -> bool {
        self.completed.succeeded()
    }
    /// The addon's refusal of this task: `engine_refused`, naming the task,
    /// with its bounded words in `detail.engine` and, when it wrote its typed
    /// refusal on stdout, that refusal in `detail.native`.
    pub fn refusal(&self) -> Failure {
        let mut failure = SOLAR.failure_from(&self.completed, self.task.subcommand());
        if !self.completed.truncated
            && self.completed.stdout.len() <= 8192
            && let Ok(native) = serde_json::from_str::<Value>(&self.completed.stdout)
            && native["status"] == "error"
        {
            let mut detail = failure.detail_value().cloned().unwrap_or_else(|| json!({}));
            detail["native"] = native["error"].clone();
            failure = failure.detail(detail);
        }
        failure
    }
}

fn unsupported(task: Task) -> Failure {
    // The literal is TASK_UNSUPPORTED, written out for the refusal-coverage scan.
    Failure::unavailable(
        "unknown_task",
        format!(
            "the installed Solar addon does not declare the {} task",
            task.name()
        ),
    )
    .remedy(format!(
        "install a Solar addon for this lane that declares {}; no in-process or alternate calculation exists",
        task.catalogue_schema()
    ))
    .detail(json!({"task": task.name(), "subcommand": task.subcommand(), "schema": task.catalogue_schema()}))
}

#[cfg(test)]
#[path = "solar_tests.rs"]
mod tests;
