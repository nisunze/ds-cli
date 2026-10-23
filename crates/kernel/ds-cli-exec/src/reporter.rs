//! The one Kernel owner of every Core → Reporter call.
//!
//! A Reporter task is a typed request file the host wrote, one result
//! document the Reporter writes, and the Reporter's own refusal. This client
//! captures exactly one lane's executable, reads that executable's identity
//! and task catalogue on demand, and refuses a task the installed Reporter
//! does not register before any work starts. Hosts never name a subcommand,
//! build an argument vector or start the executable: they choose a closed
//! [`Task`] and hand over their request and result paths.
//!
//! Compatibility is the catalogue's answer for the task actually run, never
//! a version, build date or digest equality with this Core.
use crate::{CapturedExternal, Completed, addons::REPORTER};
use ds_command_kernel::project_dataset_revisions::tasks as dataset_tasks;
use ds_command_kernel::report_export;
use ds_core_contracts::Failure;
use serde_json::{Value, json};
use std::ffi::OsString;
use std::path::Path;
use std::sync::OnceLock;
use std::time::Duration;

/// Discovery answers immediately or something is wrong.
pub const DISCOVERY_TIMEOUT: Duration = Duration::from_secs(20);
/// The stable code for a task the installed Reporter does not register (the
/// code `ds report export` and the LV A4 print already declared for it).
pub const TASK_UNSUPPORTED: &str = "unknown_task";

/// Every Reporter task Core calls, by the name its catalogue registers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Task {
    ExportTransformerReport,
    ExportCombinedTransformerReport,
    DiscoverCombinedMediaReferences,
    ExportCompoundedReport,
    ExportSpatialSectorCounts,
    ExportLvStandard,
    RenderPrintLayout,
    RenderGridPlanProfile,
    RenderGridPlanProfileModel,
    RenderMvFrontmatter,
    RenderVoltageDropResult,
    RenderVoltageDropCombined,
    PlanSolarNetworkSeed,
    DetectCollisions,
    PreparePublisherFootprints,
    PlanContourLattice,
    DeriveContours,
}

impl Task {
    /// The catalogue name (`task-schemas` `.tasks[].name`).
    pub const fn name(self) -> &'static str {
        match self {
            Self::ExportTransformerReport => "export_transformer_report",
            Self::ExportCombinedTransformerReport => "export_combined_transformer_report",
            Self::DiscoverCombinedMediaReferences => "discover_combined_media_references",
            Self::ExportCompoundedReport => "export_compounded_report",
            Self::ExportSpatialSectorCounts => "export_spatial_sector_counts",
            Self::ExportLvStandard => "export_lv_standard",
            Self::RenderPrintLayout => "render_print_layout",
            Self::RenderGridPlanProfile => "render_grid_plan_profile",
            Self::RenderGridPlanProfileModel => "render_grid_plan_profile_model",
            Self::RenderMvFrontmatter => "render_mv_frontmatter",
            Self::RenderVoltageDropResult => "render_voltage_drop_result",
            Self::RenderVoltageDropCombined => "render_voltage_drop_combined",
            Self::PlanSolarNetworkSeed => "plan_solar_network_seed",
            Self::DetectCollisions => "detect_collisions",
            Self::PreparePublisherFootprints => "prepare_publisher_footprints",
            Self::PlanContourLattice => "plan_contour_lattice",
            Self::DeriveContours => "derive_contours",
        }
    }

    /// The fixed process subcommand; only this owner passes it to a process.
    pub const fn subcommand(self) -> &'static str {
        match self {
            Self::ExportTransformerReport => report_export::REPORT_SUBCOMMAND,
            Self::ExportCombinedTransformerReport => report_export::combined::SUBCOMMAND,
            Self::DiscoverCombinedMediaReferences => report_export::combined::MEDIA_SUBCOMMAND,
            Self::ExportCompoundedReport => "export-compounded-report",
            Self::ExportSpatialSectorCounts => "export-spatial-sector-counts",
            Self::ExportLvStandard => "export-lv-standard",
            Self::RenderPrintLayout => "render-print-layout",
            Self::RenderGridPlanProfile => "render-grid-plan-profile",
            Self::RenderGridPlanProfileModel => "render-grid-plan-profile-model",
            Self::RenderMvFrontmatter => "render-mv-frontmatter",
            Self::RenderVoltageDropResult => "render-voltage-drop-result",
            Self::RenderVoltageDropCombined => "render-voltage-drop-combined",
            Self::PlanSolarNetworkSeed => "plan-solar-network-seed",
            Self::DetectCollisions => "detect-collisions",
            Self::PreparePublisherFootprints => dataset_tasks::FOOTPRINTS_SUBCOMMAND,
            Self::PlanContourLattice => dataset_tasks::CONTOUR_PLAN_SUBCOMMAND,
            Self::DeriveContours => dataset_tasks::CONTOURS_SUBCOMMAND,
        }
    }
}

/// One accepted run's Reporter: the captured executable of one lane, its
/// catalogue read at most once, and every task invoked through it.
pub struct Reporter {
    engine: CapturedExternal,
    catalogue: OnceLock<Value>,
}

/// The Reporter's answer to one task: its exit and bounded streams. The
/// result document stays at the host's result path, which the host reads
/// against its own typed receipt.
pub struct Reply {
    task: Task,
    completed: Completed,
}

/// Discovery: a Reporter is installed for some lane (or the development
/// override names one). Commands that always run the Reporter declare this,
/// so `ds capabilities` agrees with what invocation finds.
pub fn available() -> ds_core_contracts::Availability {
    REPORTER.availability_for_any_lane()
}

impl Reporter {
    /// Capture the explicit installed lane (or the deliberate development override).
    pub fn capture_for_lane(lane: &str) -> Result<Self, Failure> {
        REPORTER.capture_for_lane(lane).map(Self::from_captured)
    }

    /// Native-owned relocated addon store, never a command-supplied path.
    pub fn capture_for_lane_in(lane: &str, base: &Path) -> Result<Self, Failure> {
        REPORTER
            .capture_for_lane_in(lane, base)
            .map(Self::from_captured)
    }

    /// Reopen one exact executable a retained run or a test fixture pinned.
    pub fn capture_pinned(path: &Path) -> Result<Self, Failure> {
        REPORTER.capture_pinned(path).map(Self::from_captured)
    }

    fn from_captured(engine: CapturedExternal) -> Self {
        Self {
            engine,
            catalogue: OnceLock::new(),
        }
    }

    /// The canonical executable this run captured; never resolved again.
    pub fn executable(&self) -> &Path {
        self.engine.executable()
    }

    /// The captured Reporter's own identity document (`build-info`).
    pub fn build_info(&self) -> Result<Value, Failure> {
        self.engine.call_json("build-info", &[], DISCOVERY_TIMEOUT)
    }

    /// The captured Reporter's registered tasks and request schemas, read once.
    pub fn task_catalogue(&self) -> Result<&Value, Failure> {
        if let Some(catalogue) = self.catalogue.get() {
            return Ok(catalogue);
        }
        let read = self
            .engine
            .call_json("task-schemas", &[], DISCOVERY_TIMEOUT)?;
        if !read["tasks"].is_array() {
            return Err(Failure::failed(
                "callee_contract_mismatch",
                "the captured Reporter returned no task catalogue",
            )
            .remedy("install a Reporter that publishes its task catalogue"));
        }
        Ok(self.catalogue.get_or_init(|| read))
    }

    /// Refuse, before any work, a task this Reporter does not register under
    /// the fixed subcommand this client invokes. An entry that names another
    /// task for that subcommand does not register it; members this client
    /// does not use (including an absent name) are ignored.
    pub fn require(&self, task: Task) -> Result<&Value, Failure> {
        let catalogue = self.task_catalogue()?;
        catalogue["tasks"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|entry| {
                entry["subcommand"] == task.subcommand()
                    && entry.get("name").is_none_or(|name| name == task.name())
            })
            .ok_or_else(|| unsupported(task))
    }

    /// Run one registered task over the host's typed request and result
    /// files. The capability check precedes the process; the receipt check of
    /// a managed executable brackets it.
    pub fn run(
        &self,
        task: Task,
        request: &Path,
        result: &Path,
        timeout: Duration,
    ) -> Result<Reply, Failure> {
        self.require(task)?;
        let completed =
            self.engine
                .call(task.subcommand(), &file_arguments(request, result), timeout)?;
        Ok(Reply { task, completed })
    }

    /// Start one registered task over host-owned durable paths and return at
    /// once; the Reporter owns its lock and failure journal, so leaving this
    /// process never cancels it. Streams are discarded; the result file is
    /// the only answer.
    pub fn launch(&self, task: Task, request: &Path, result: &Path) -> Result<(), Failure> {
        self.require(task)?;
        self.engine
            .launch_detached(task.subcommand(), &file_arguments(request, result))
    }
}

impl Reply {
    pub fn task(&self) -> Task {
        self.task
    }
    pub fn succeeded(&self) -> bool {
        self.completed.succeeded()
    }
    pub fn status(&self) -> Option<i32> {
        self.completed.status
    }
    pub fn stdout(&self) -> &str {
        &self.completed.stdout
    }
    pub fn stderr(&self) -> &str {
        &self.completed.stderr
    }
    pub fn truncated(&self) -> bool {
        self.completed.truncated
    }
    /// The Reporter's refusal of this task: `engine_refused`, naming the task,
    /// with its own bounded message in `detail.engine`.
    pub fn refusal(&self) -> Failure {
        REPORTER.failure_from(&self.completed, self.task.subcommand())
    }
}

fn file_arguments(request: &Path, result: &Path) -> [OsString; 4] {
    [
        "--request".into(),
        request.into(),
        "--result".into(),
        result.into(),
    ]
}

fn unsupported(task: Task) -> Failure {
    // The literal is TASK_UNSUPPORTED, written out for the refusal-coverage scan.
    Failure::unavailable(
        "unknown_task",
        format!(
            "the installed Reporter does not register the {} task",
            task.name()
        ),
    )
    .remedy(format!(
        "install a Reporter for this lane that registers {}; no alternate rendering or recomputation path exists",
        task.name()
    ))
    .detail(json!({"task": task.name(), "subcommand": task.subcommand()}))
}

#[cfg(test)]
#[path = "reporter_tests.rs"]
mod tests;
