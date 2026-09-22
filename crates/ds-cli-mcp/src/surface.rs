//! MCP publication shapes generated from the live command descriptors.

use std::path::PathBuf;

use ds_cli_contract::outcome::{Failure, error_envelope};
use ds_cli_contract::spec::Chapter;
use serde_json::{Map, Value, json};

use crate::tools::{self, CONFIRM_PROPERTY, Tool};

pub const EXPOSURES: &[&str] = &["chapters", "commands"];

// MCP never asks an operator for a password. The protected device-link commands
// remain the only sign-in path this surface advertises.
const DEVICE_LINK_GUIDANCE: &str = "If signed out, call auth.link.begin on this host, approve its request from the signed-in Desktop, then call auth.link.complete. Keep the same lane and principal throughout. Never collect an email or password through MCP.";
const DEVICE_LINK_REMEDY: &str = "Use the protected device link: auth.link.begin, approval from the signed-in Desktop, then auth.link.complete on this host.";
const PASSWORD_ONLY_REMEDY: &str = "This command currently requires a password session and cannot run with a device-linked MCP identity; report this command as a device-link coverage gap.";
pub const PROFILE_IDS: &[&str] = &[
    "auth-context",
    "admin-bounds",
    "grid",
    "grid-native",
    "printing",
    "grid-local-model",
    "clearance",
    "pls",
    "pls-library",
    "library-governance",
    "survey",
    "form-factory",
    "survey-projects",
    "survey-media",
    "survey-migration",
    "design-edit",
    "design-run",
    "map",
    "styles",
    "print-styles",
    "layers",
    "tiling",
    "project",
    "solar-input",
    "solar-migration",
    "design-migration",
    "solar-application",
    "solar-dashboard",
    "solar-run",
    "solar-delivery",
    "solar-portfolio-batch",
    "operations",
    "project-operations",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Exposure {
    Chapters,
    Commands,
}

impl Exposure {
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "chapters" => Some(Self::Chapters),
            "commands" => Some(Self::Commands),
            _ => None,
        }
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::Chapters => "chapters",
            Self::Commands => "commands",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    AuthContext,
    AdminBounds,
    Installations,
    Grid,
    GridNative,
    Printing,
    GridLocalModel,
    GridClearance,
    Pls,
    PlsLibrary,
    LibraryGovernance,
    Survey,
    FormFactory,
    SurveyProjects,
    SurveyMedia,
    SurveyMigration,
    DesignEdit,
    DesignRun,
    Map,
    Styles,
    PrintStyles,
    Layers,
    Tiling,
    Project,
    SolarInput,
    SolarMigration,
    DesignMigration,
    SolarApplication,
    SolarDashboard,
    SolarRun,
    SolarDelivery,
    SolarPortfolioBatch,
    Operations,
    ProjectOperations,
}

impl Profile {
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "auth-context" => Some(Self::AuthContext),
            "admin-bounds" => Some(Self::AdminBounds),
            "installations" => Some(Self::Installations),
            "grid" => Some(Self::Grid),
            "grid-native" => Some(Self::GridNative),
            "printing" => Some(Self::Printing),
            "grid-local-model" => Some(Self::GridLocalModel),
            "clearance" => Some(Self::GridClearance),
            "pls" => Some(Self::Pls),
            "pls-library" => Some(Self::PlsLibrary),
            "library-governance" => Some(Self::LibraryGovernance),
            "survey" => Some(Self::Survey),
            "form-factory" => Some(Self::FormFactory),
            "survey-projects" => Some(Self::SurveyProjects),
            "survey-media" => Some(Self::SurveyMedia),
            "survey-migration" => Some(Self::SurveyMigration),
            "design-edit" => Some(Self::DesignEdit),
            "design-run" => Some(Self::DesignRun),
            "map" => Some(Self::Map),
            "styles" => Some(Self::Styles),
            "print-styles" => Some(Self::PrintStyles),
            "layers" => Some(Self::Layers),
            "tiling" => Some(Self::Tiling),
            "project" => Some(Self::Project),
            "solar-input" => Some(Self::SolarInput),
            "solar-migration" => Some(Self::SolarMigration),
            "design-migration" => Some(Self::DesignMigration),
            "solar-application" => Some(Self::SolarApplication),
            "solar-dashboard" => Some(Self::SolarDashboard),
            "solar-run" => Some(Self::SolarRun),
            "solar-delivery" => Some(Self::SolarDelivery),
            "solar-portfolio-batch" => Some(Self::SolarPortfolioBatch),
            "operations" => Some(Self::Operations),
            "project-operations" => Some(Self::ProjectOperations),
            _ => None,
        }
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::AuthContext => "auth-context",
            Self::AdminBounds => "admin-bounds",
            Self::Installations => "installations",
            Self::Grid => "grid",
            Self::GridNative => "grid-native",
            Self::Printing => "printing",
            Self::GridLocalModel => "grid-local-model",
            Self::GridClearance => "clearance",
            Self::Pls => "pls",
            Self::PlsLibrary => "pls-library",
            Self::LibraryGovernance => "library-governance",
            Self::Survey => "survey",
            Self::FormFactory => "form-factory",
            Self::SurveyProjects => "survey-projects",
            Self::SurveyMedia => "survey-media",
            Self::SurveyMigration => "survey-migration",
            Self::DesignEdit => "design-edit",
            Self::DesignRun => "design-run",
            Self::Map => "map",
            Self::Styles => "styles",
            Self::PrintStyles => "print-styles",
            Self::Layers => "layers",
            Self::Tiling => "tiling",
            Self::Project => "project",
            Self::SolarInput => "solar-input",
            Self::SolarMigration => "solar-migration",
            Self::DesignMigration => "design-migration",
            Self::SolarApplication => "solar-application",
            Self::SolarDashboard => "solar-dashboard",
            Self::SolarRun => "solar-run",
            Self::SolarDelivery => "solar-delivery",
            Self::SolarPortfolioBatch => "solar-portfolio-batch",
            Self::Operations => "operations",
            Self::ProjectOperations => "project-operations",
        }
    }

    const fn tool_limit(self) -> usize {
        match self {
            // The broad Grid chapter router now also carries the paired
            // application's local model lifecycle and its one project
            // publication. Native `dsgrid.create` adds the missing entry into
            // file editing: eighteen leaves plus bootstrap, one more than the
            // previous budget. Focused callers use `grid-native` (file work)
            // or `grid-local-model` (paired lifecycle).
            // Project list/download add two native asset reads to this
            // model workflow; no arbitrary download route is exposed.
            // 2026-09-20 (contract 02): three more leaves — `dsgrid model
            // show|link` and `dsgrid-exchange sync` — the working copy's
            // record, its pin to a live PLS-CADD workspace, and the write
            // back into that workspace. Without them the broad router could
            // import from PLS-CADD but never deliver to it.
            // 2026-09-21: immutable native structure import and atomic
            // apply-batch add two file-authoring leaves to this same workflow.
            Self::Grid => 27,
            // The two reference-form commands add manual/shared seeding to
            // this input workflow; the legacy planner remains discoverable.
            // City creation adds the missing editable draft entry point,
            // including cities without geographic or classified source data.
            Self::SolarInput => 18,
            // Query, spatial selection, fenced changes, and governed
            // single-entry create belong to the same selected-project Survey
            // workflow. The count includes both bootstrap tools.
            // Three working-area leaves read, choose and forget which forms
            // the map loads for the project — a local choice, no fetch. The
            // photo leaves are `survey-media`'s (2026-09-20).
            Self::SurveyProjects => 21,
            // Twenty-one governed design-edit leaves plus the two bootstrap
            // tools. Version history and the pinned Working set project the
            // same bounded desktop-owned workflow without transporting
            // features through MCP.
            Self::DesignEdit => 23,
            // Twenty-three printing leaves plus both bootstrap tools: city
            // vector acquisition adds one step to the existing headless
            // printing workflow, including projects without held context. The
            // headless loop (context held and seeded on this machine, output
            // selection, export with context) beside the paired leaves that
            // still need the desktop's own holdings.
            // Map delivery adds local composition, standalone publication,
            // and the published map index. General asset referencing remains
            // discoverable in the Assets chapter, outside this print workflow.
            Self::Printing => 28,
            // The layer drawer's profile also carries this machine's prepared
            // local layer catalogue: seventeen leaves plus both bootstrap tools.
            // Preparing, renaming and removing a local layer is the same
            // operator workflow as the local tile references beside it — one
            // host's own layers — so it is not a second profile.
            Self::Layers => 19,
            // Sixteen leaves plus both bootstrap tools. The one that raised
            // this from seventeen on 2026-09-18 is `ds feedback note`: with
            // three feedback verbs the only way to say anything about a report
            // was to close it, so a report parked on a deploy or a ruling left
            // no record and every later visit re-read its full text to
            // rediscover the same blocker. An operations profile that can read
            // the backlog and close a report, but cannot say why a report it
            // touched stays open, forces the next reader to rescan everything.
            // The one before that is `ds desktop list`: with several DS
            // GridDesign instances live on a machine, every instance-targeted
            // refusal tells the caller to name one, and this is the only tool
            // that says which exist. A profile that could refuse an operation
            // for ambiguity and not publish the answer to it would not be a
            // smaller surface, only a stuck one.
            Self::Operations => 18,
            // Sixteen leaves plus both bootstrap tools. Raised from the
            // default on 2026-09-19 by the publication queue: an agent doing
            // background delivery work produces report artifacts, and a
            // profile that can produce them but cannot publish them or read
            // where they are strands its own output on the machine. Raised
            // again on 2026-09-20 by `report.project.compute`: the same
            // individual report produced in the cloud, which is how edge and
            // cloud production are proven to meet in one project.
            Self::ProjectOperations => 18,
            // Seventeen leaves plus both bootstrap tools. Raised from the
            // default on 2026-09-20 by the three task-geometry leaves (`pm task
            // geometry read|set|clear`): an agent that reads a comment naming
            // structures 74, 76, 77 in a swamp and can create the task, but
            // cannot say WHERE it is, leaves the plan and the map without the
            // one thing the comment was about. The proposal is what the person
            // confirms; the read is what the map paints from.
            Self::Project => 19,
            _ => 16,
        }
    }

    pub fn includes(self, tool: &Tool) -> bool {
        match self {
            Self::AuthContext => AUTH_CONTEXT_COMMANDS.contains(&tool.id.as_str()),
            Self::AdminBounds => ADMIN_BOUNDS_COMMANDS.contains(&tool.id.as_str()),
            Self::Installations => INSTALLATION_COMMANDS.contains(&tool.id.as_str()),
            Self::GridNative => {
                tool.authority == ds_cli_contract::spec::Authority::None
                    && (tool.id.starts_with("dsgrid.") || tool.id.starts_with("dsgrid-exchange."))
                    // The working-copy family became authority-free on
                    // 2026-09-18 when it stopped asking an application for
                    // this machine's catalogue. It is still a different job
                    // from the file-in/file-out engine workflow, and it keeps
                    // its own profile.
                    && !GRID_LOCAL_MODEL_COMMANDS.contains(&tool.id.as_str())
                    // The feature-code and clearance workflow (program
                    // contract 03) is one operator job over a working copy
                    // and keeps its own profile, like the local-model family.
                    && !GRID_CLEARANCE_COMMANDS.contains(&tool.id.as_str())
            }
            Self::Grid => {
                matches!(tool.chapter, Chapter::GridModel | Chapter::Reports)
                    && !PROJECT_OPERATIONS_COMMANDS.contains(&tool.id.as_str())
                    // Printing has its own workflow profile and Reports router;
                    // changing that profile must not expand the Grid surface.
                    && !PRINTING_COMMANDS.contains(&tool.id.as_str())
                    && !tool.id.starts_with("desktop.printing.")
                    && !tool.id.starts_with("report.layout.")
                    // Project head preparation (this machine's working copies
                    // against the governed heads) belongs to the focused model
                    // lifecycle surface. Keeping it out of this broad router
                    // preserves the profile's bounded tool budget.
                    && tool.id != "dsgrid.model.prepare-project"
                    // So do the working copy's typed edits and its structure
                    // list (`grid-local-model`, 2026-09-20).
                    && !GRID_LOCAL_MODEL_TYPED_EDITS.contains(&tool.id.as_str())
                    // And the feature-code / clearance workflow (program
                    // contract 03, 2026-09-20): its own profile, `clearance`.
                    && !GRID_CLEARANCE_COMMANDS.contains(&tool.id.as_str())
                    // Deleted LV snapshot recovery is an Assets workflow,
                    // outside this engineering-model profile. The global
                    // catalogue and Grid Model chapter retain its command.
                    && tool.id != "dsgrid.backup.preview"
            }
            Self::Printing => PRINTING_COMMANDS.contains(&tool.id.as_str()),
            Self::GridLocalModel => GRID_LOCAL_MODEL_COMMANDS.contains(&tool.id.as_str()),
            Self::GridClearance => GRID_CLEARANCE_COMMANDS.contains(&tool.id.as_str()),
            Self::Pls => tool.chapter == Chapter::PlsCadd && tool.id.starts_with("pls."),
            Self::PlsLibrary => PLS_LIBRARY_COMMANDS.contains(&tool.id.as_str()),
            Self::LibraryGovernance => LIBRARY_GOVERNANCE_COMMANDS.contains(&tool.id.as_str()),
            Self::Survey => SURVEY_MAP_COMMANDS.contains(&tool.id.as_str()),
            Self::FormFactory => FORM_FACTORY_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyProjects => SURVEY_PROJECT_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyMedia => SURVEY_MEDIA_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyMigration => SURVEY_MIGRATION_COMMANDS.contains(&tool.id.as_str()),
            Self::Map => {
                tool.chapter == Chapter::MapPresentation
                    && !STYLE_COMMANDS.contains(&tool.id.as_str())
                    && !PRINT_STYLE_COMMANDS.contains(&tool.id.as_str())
            }
            Self::Styles => STYLE_COMMANDS.contains(&tool.id.as_str()),
            Self::PrintStyles => PRINT_STYLE_COMMANDS.contains(&tool.id.as_str()),
            Self::Layers => LAYER_COMMANDS.contains(&tool.id.as_str()),
            Self::Tiling => tool.chapter == Chapter::VectorTiles,
            // Native account bootstrap is available on the broad live surface
            // but is not project-workflow tooling and must not inflate the
            // already bounded specialized project profile.
            Self::Project => tool.chapter == Chapter::Project && !tool.id.starts_with("auth."),
            Self::Operations => {
                tool.chapter == Chapter::Operations
                    && !INSTALLATION_COMMANDS.contains(&tool.id.as_str())
            }
            Self::DesignEdit => DESIGN_EDIT_COMMANDS.contains(&tool.id.as_str()),
            Self::DesignRun => DESIGN_RUN_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarInput => SOLAR_INPUT_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarMigration => SOLAR_MIGRATION_COMMANDS.contains(&tool.id.as_str()),
            Self::DesignMigration => DESIGN_MIGRATION_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarDashboard => SOLAR_DASHBOARD_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarRun => SOLAR_RUN_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarApplication => SOLAR_APPLICATION_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarDelivery => SOLAR_DELIVERY_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarPortfolioBatch => SOLAR_PORTFOLIO_BATCH_COMMANDS.contains(&tool.id.as_str()),
            Self::ProjectOperations => PROJECT_OPERATIONS_COMMANDS.contains(&tool.id.as_str()),
        }
    }

    /// The exact command ids a by-command profile publishes, or an empty
    /// slice for a profile that selects by chapter.
    ///
    /// Exposed so a test holding the live registry can prove these
    /// hand-written splits still partition it. Chapter membership is declared
    /// once, on the command; split workflow profiles are not, and an unlisted command in a
    /// split chapter is simply unreachable through its profile — silently,
    /// and with every unit test still passing.
    pub const fn command_ids(self) -> &'static [&'static str] {
        match self {
            Self::AuthContext => AUTH_CONTEXT_COMMANDS,
            Self::AdminBounds => ADMIN_BOUNDS_COMMANDS,
            Self::Installations => INSTALLATION_COMMANDS,
            Self::Printing => PRINTING_COMMANDS,
            Self::GridLocalModel => GRID_LOCAL_MODEL_COMMANDS,
            Self::GridClearance => GRID_CLEARANCE_COMMANDS,
            Self::Survey => SURVEY_MAP_COMMANDS,
            Self::FormFactory => FORM_FACTORY_COMMANDS,
            Self::SurveyProjects => SURVEY_PROJECT_COMMANDS,
            Self::SurveyMedia => SURVEY_MEDIA_COMMANDS,
            Self::SurveyMigration => SURVEY_MIGRATION_COMMANDS,
            Self::Layers => LAYER_COMMANDS,
            Self::Styles => STYLE_COMMANDS,
            Self::PrintStyles => PRINT_STYLE_COMMANDS,
            Self::DesignEdit => DESIGN_EDIT_COMMANDS,
            Self::DesignRun => DESIGN_RUN_COMMANDS,
            Self::SolarInput => SOLAR_INPUT_COMMANDS,
            Self::SolarMigration => SOLAR_MIGRATION_COMMANDS,
            Self::DesignMigration => DESIGN_MIGRATION_COMMANDS,
            Self::SolarDashboard => SOLAR_DASHBOARD_COMMANDS,
            Self::SolarRun => SOLAR_RUN_COMMANDS,
            Self::SolarApplication => SOLAR_APPLICATION_COMMANDS,
            Self::SolarDelivery => SOLAR_DELIVERY_COMMANDS,
            Self::SolarPortfolioBatch => SOLAR_PORTFOLIO_BATCH_COMMANDS,
            Self::PlsLibrary => PLS_LIBRARY_COMMANDS,
            Self::LibraryGovernance => LIBRARY_GOVERNANCE_COMMANDS,
            Self::ProjectOperations => PROJECT_OPERATIONS_COMMANDS,
            Self::Grid
            | Self::GridNative
            | Self::Pls
            | Self::Map
            | Self::Tiling
            | Self::Project
            | Self::Operations => &[],
        }
    }

    pub fn includes_chapter(self, chapter: Chapter) -> bool {
        match self {
            Self::AuthContext => chapter == Chapter::Project,
            Self::AdminBounds => chapter == Chapter::Data,
            Self::Installations => chapter == Chapter::Operations,
            Self::Grid => matches!(chapter, Chapter::GridModel | Chapter::Reports),
            Self::GridNative => chapter == Chapter::GridModel,
            Self::Printing => matches!(chapter, Chapter::Reports | Chapter::MapPresentation),
            Self::GridLocalModel | Self::GridClearance => chapter == Chapter::GridModel,
            Self::Pls | Self::PlsLibrary | Self::LibraryGovernance => chapter == Chapter::PlsCadd,
            Self::Survey
            | Self::FormFactory
            | Self::SurveyProjects
            | Self::SurveyMedia
            | Self::SurveyMigration
            | Self::Layers => chapter == Chapter::Survey,
            Self::DesignEdit | Self::DesignRun | Self::DesignMigration => {
                chapter == Chapter::Design
            }
            Self::Map | Self::Styles | Self::PrintStyles => chapter == Chapter::MapPresentation,
            Self::Tiling => chapter == Chapter::VectorTiles,
            Self::Project => chapter == Chapter::Project,
            Self::SolarInput
            | Self::SolarMigration
            | Self::SolarApplication
            | Self::SolarDashboard
            | Self::SolarRun
            | Self::SolarDelivery
            | Self::SolarPortfolioBatch => chapter == Chapter::Solar,
            Self::Operations => chapter == Chapter::Operations,
            Self::ProjectOperations => matches!(chapter, Chapter::Design | Chapter::Reports),
        }
    }
}

// Principal handoff for an MCP host uses only the protected native session a
// person established in a trusted terminal. Password login is intentionally
// absent: an MCP child may inspect the non-secret AuthContext, refresh the
// visible project directory, and select one exact visible project, but it may
// never receive password, approval authority, or credential material. Device
// begin/status/complete and inventory operate only through protected native
// state; `auth.link.approve` remains human-only and globally excluded.
const AUTH_CONTEXT_COMMANDS: &[&str] = &[
    "auth.status",
    "auth.link.begin",
    "auth.link.status",
    "auth.link.complete",
    "auth.device.list",
    "auth.device.read",
    "auth.device.revoke",
    "auth.project.list",
    "auth.project.use",
    "auth.project.status",
];

/// The installation inventory is its own operator workflow, not part of the
/// reliability and host tooling beside it. Somebody asking which installations
/// exist and whose licence is blocked is doing one job; somebody reading fleet
/// health or driving a local host is doing another. Splitting them keeps both
/// surfaces small and keeps the Operations chapter inside its ceiling.
const INSTALLATION_COMMANDS: &[&str] = &[
    "install.list",
    "install.show",
    "install.policy",
    "install.retire",
];

const ADMIN_BOUNDS_COMMANDS: &[&str] = &[
    "data.admin-bounds.list",
    "data.admin-bounds.read",
    "data.admin-bounds.attach",
];

/// Guided Style Center workflows have their own bounded MCP profile. Keeping
/// them out of the general map profile prevents each authoring axis from
/// silently widening map navigation and evidence tooling.
const STYLE_COMMANDS: &[&str] = &[
    "style.list",
    "style.read",
    "style.appearance.plan",
    "style.appearance.set",
    "style.label.plan",
    "style.label.set",
    "style.dimension.plan",
    "style.dimension.set",
    "style.dimension.clear",
    "style.cartography.plan",
    "style.cartography.set",
];

/// Create-only source seeding and print cloning are one small publication
/// workflow, kept separate from ordinary Style Center edits.
const PRINT_STYLE_COMMANDS: &[&str] = &[
    "style.seed.plan",
    "style.seed.create",
    "style.print.plan",
    "style.print.create",
];

// The paired application's DS Grid model lifecycle, in the order the work
// happens. Deliberately narrow: an agent acquiring a model, choosing which one
// occupies Profile and publishing one revision needs these five leaves and
// nothing else, and the four local ones reach no project at all. The one
// project act is published beside them because it is where the workflow ends,
// and it stays confirmation-gated exactly as the CLI declares it.
const GRID_LOCAL_MODEL_COMMANDS: &[&str] = &[
    "dsgrid.model.list",
    "dsgrid.model.show",
    "dsgrid.model.create-local",
    "dsgrid.model.import-external",
    // The link to a live PLS-CADD workspace is a fact about this machine's
    // working copy, so it lives with the copy's lifecycle; the sync that
    // uses it is file-in/file-out engine work and stays native.
    "dsgrid.model.link",
    "dsgrid.model.set-active",
    // The paired application shows a working copy in Profile (2026-09-20):
    // the one door from this machine's catalogue into the window.
    "dsgrid.profile.open",
    "dsgrid.model.prepare-project",
    "dsgrid.publish-version",
    // The typed edits of a working copy and its structure list (program
    // contract 01 §2, 2026-09-20) are the working-copy workflow: read the
    // head, describe, retype, list. They live here beside the copy they edit
    // rather than widening the broad `grid` router past its budget, exactly
    // as `prepare-project` does; the chapter router carries them regardless.
    "dsgrid.structure.describe",
    "dsgrid.structure.retype",
    "dsgrid.structure.staking-enrich",
    "dsgrid.report.structures",
];

/// The members of `grid-local-model` that the broad `grid` router leaves to
/// it (its budget holds the file-in/file-out engine workflow).
const GRID_LOCAL_MODEL_TYPED_EDITS: &[&str] = &[
    "dsgrid.model.show",
    "dsgrid.profile.open",
    "dsgrid.structure.describe",
    "dsgrid.structure.retype",
    "dsgrid.structure.staking-enrich",
    "dsgrid.report.structures",
];

// Program contract 03: feature codes and clearance across the PLS-CADD
// boundary — the table report, the standard import, the migration of survey
// tokens, the FEA export, the clearance criteria (set and show) and the
// clearance report. One operator workflow over one working copy, from the
// consultant's "clearances not configured" to the list of violations by
// feature code; the surrounding file workflow stays in `grid-native`.
const GRID_CLEARANCE_COMMANDS: &[&str] = &[
    "dsgrid.feature-codes.report",
    "dsgrid.feature-codes.import",
    "dsgrid.feature-codes.migrate",
    "dsgrid.feature-codes.export",
    "dsgrid.criteria.show",
    "dsgrid.criteria.clearance.set",
    "dsgrid.analyse.clearance",
];

const PLS_LIBRARY_COMMANDS: &[&str] = &[
    "library.verify",
    "library.open",
    "library.catalog",
    "library.pack",
    "library.prepare-publication",
    "library.unpack",
    "library.seed",
    "library.resolve-native",
];

const LIBRARY_GOVERNANCE_COMMANDS: &[&str] = &[
    "library.global.read",
    "library.global.write",
    "library.global.fork-example",
    "library.global.upload",
    "library.global.publish-library",
    "library.global.publish-example",
    "library.global.library-lifecycle",
    "library.global.example-lifecycle",
];

const SURVEY_MAP_COMMANDS: &[&str] = &[
    "map.view",
    "map.draw",
    "map.remove",
    "map.zoom",
    "map.ui.open",
    "map.evidence.capture",
    "map.points-along",
    "map.random-points",
    "map.outliers",
    "map.line-difference",
    "map.survey.download",
    "map.survey.migrate.plan",
    "map.survey.migrate.apply",
];

const LAYER_COMMANDS: &[&str] = &[
    "map.layer.list",
    "map.layer.default",
    "map.layer.reorder",
    "map.layer.remote-list",
    "map.data.inspect",
    "map.data.list",
    "map.data.upload",
    "map.data.remove",
    "map.layer.add",
    "map.layer.remove",
    "map.layer.visibility",
    "map.layer.show",
    "map.layer.hide",
    "map.local.list",
    "map.local.register",
    "map.local.rename",
    "map.local.remove",
];

const FORM_FACTORY_COMMANDS: &[&str] = &[
    "survey.forms.list",
    "survey.form.read",
    "survey.form.types",
    "survey.form.create",
    "survey.form.update",
    "survey.form.lifecycle",
];

// Survey photos as one operator workflow: what this machine holds, one
// photo's details, the one rotation (held locally first) and its
// publication, plus the offline file rotation. Split out of
// `survey-projects` on 2026-09-20 when the moments leaves would have taken
// that profile past its bound.
const SURVEY_MEDIA_COMMANDS: &[&str] = &[
    "survey.moments.list",
    "survey.moments.read",
    "survey.photo.rotate",
    "survey.photo.publish",
    "survey.photo.rotate-local",
];

const SURVEY_PROJECT_COMMANDS: &[&str] = &[
    "survey.query",
    "survey.entries.select",
    "survey.entries.changes",
    "survey.entries.create",
    "survey.project-forms.list",
    "survey.project-form.settings",
    "survey.project-forms.read",
    "survey.project-form.editor",
    "survey.project-forms.plan",
    "survey.project-forms.apply",
    "survey.working-area.forms",
    "survey.working-area.select",
    "survey.working-area.clear",
    "survey.templates.list",
    "survey.template.read",
    "survey.template.create",
    "survey.template.apply",
    "survey.template.lifecycle",
    "survey.project.create-from-template",
];

// Canonical NDJSON import is the supported Survey migration workflow.
const SURVEY_MIGRATION_COMMANDS: &[&str] = &["survey.entries.import"];

const DESIGN_EDIT_COMMANDS: &[&str] = &[
    "design.features.select",
    "design.known-columns.list",
    "design.known-columns.set",
    "map.design.open",
    "map.design.pin",
    "map.design.read",
    "map.design.discard",
    "map.design.layer-to-local",
    "map.design.upload-to-local",
    "map.design.select",
    "map.design.set",
    "map.design.create",
    "map.design.delete",
    "map.design.geometry",
    "map.design.setup",
    "map.design.version.play",
    "map.design.version.compare",
    "map.design.upload.inspect",
    "map.design.upload.stage",
];

// Background project operations: no map or room activation. Retirement and
// reports use the CLI-selected headless project. (`design.transformer.download`
// left on 2026-09-20: it only ever warmed a window's private room cache, and
// the native report path reads rooms from the service.) Kept out of
// `design-edit` (already at its bound) and out of the `grid` chapter router
// so neither grows; an agent doing background delivery work gets this narrow
// profile.
const PROJECT_OPERATIONS_COMMANDS: &[&str] = &[
    "design.status",
    "design.transformer.inventory",
    "design.transformer.retire",
    "design.transformer.restore",
    "report.project.scope",
    "report.project.settings",
    "report.project.outputs.set",
    "report.project.combined",
    // The retired id, reachable for one release so an agent holding the old
    // name still finds the Combined Report.
    "report.project.compounded",
    "report.project.archives",
    "report.project.export",
    // The cloud twin of `export`: the same individual report, computed and
    // published server-side. Listed here so the broad `grid` chapter router,
    // which excludes this list, does not grow by one.
    "report.project.compute",
    // Publishing a set this machine already holds, and the queue every one of
    // those exports enters. Without them this profile can PRODUCE report
    // artifacts and cannot publish them or say where they are — which is the
    // defect the publication queue exists to end, reproduced inside one
    // profile. They are listed here so the broad `grid` chapter router, which
    // excludes this list, does not grow by three.
    "report.project.publish",
    "report.outbox.status",
    "report.outbox.drain",
];

const DESIGN_RUN_COMMANDS: &[&str] = &[
    "design.intake.upload",
    "design.lv.project-export",
    "design.lv.process",
    "map.design.process",
    "map.design.batch.process",
    "map.design.batch.report",
    "map.design.batch.save",
    "map.design.save",
    "map.design.list",
    "map.design.report",
    "map.design.attach-print",
];

const SOLAR_APPLICATION_COMMANDS: &[&str] = &["solar.application", "solar.application.schema"];
const SOLAR_RUN_COMMANDS: &[&str] = &[
    "solar.engine",
    // Preserve the established end-to-end run profile. Native governed input
    // handoffs get their own narrow profile because adding them here would
    // exceed the bounded leaf-tool surface and silently change existing hosts.
    "solar.seed.preview",
    "solar.seed.apply",
    "solar.prepare",
    "solar.run",
    "solar.run.start",
    "solar.run.progress",
    "solar.run.result",
    "solar.run.cancel",
    "solar.result.compare",
    "solar.result.read",
    "solar.results.read",
    "solar.sync.status",
    "solar.verify-weather",
];

const SOLAR_DASHBOARD_COMMANDS: &[&str] = &[
    "solar.engine",
    "solar.results.read",
    "solar.dashboard.compose",
    "solar.project.result",
];

const SOLAR_INPUT_COMMANDS: &[&str] = &[
    "solar.network.resolve",
    "solar.network.save",
    "solar.network.map",
    "solar.cities",
    "solar.reference.acquire",
    "solar.project.run",
    "solar.project.result",
    "solar.project.status",
    "solar.project.init",
    "solar.project.seed",
    "solar.project.city.create",
    "solar.project.city.read",
    "solar.project.city.write",
    "solar.input.capture",
    "solar.input.prepare",
    "solar.seed.network-plan",
];

/// Moving a project's Solar inputs into another project is its own operator
/// workflow — plan it, read what will and will not move, confirm the digest —
/// and it is not the workflow of authoring a city's inputs. `solar-input` and
/// `solar-run` are both already at their bounded leaf-tool surface, and a
/// profile that grew to carry a second workflow would silently change every
/// host already using it. This mirrors `survey-migration`, which is a separate
/// profile for exactly the same reason.
const SOLAR_MIGRATION_COMMANDS: &[&str] = &["solar.migrate.plan", "solar.migrate.apply"];

// Each domain's migration is its own narrow operator workflow, exactly as
// `survey-migration` and `solar-migration` are. Plan and apply travel
// together: an apply whose plan an agent cannot reach is an apply nobody
// reviewed. Kept out of `project-operations`, which is already at its bound.
const DESIGN_MIGRATION_COMMANDS: &[&str] = &["design.migrate.plan", "design.migrate.apply"];

const SOLAR_PORTFOLIO_BATCH_COMMANDS: &[&str] = &[
    "solar.portfolio.calculate",
    "solar.portfolio.publish",
    "solar.portfolio.published.read",
    "solar.portfolio.batch.start",
    "solar.portfolio.batch.status",
    "solar.portfolio.batch.cancel",
];

const SOLAR_DELIVERY_COMMANDS: &[&str] = &[
    "solar.project.outbox",
    "solar.project.sync",
    "solar.project.sync.rebase",
    "solar.portfolio.list",
    "solar.portfolio.create",
    "solar.portfolio.update",
    "solar.portfolio.delete",
    "solar.portfolio.read",
    "solar.portfolio.analysis",
    "solar.final.import",
    "solar.final.submit",
    "solar.report.export",
    "solar.report.bundle",
    "solar.portfolio.export",
];

/// Every chapter except `Catalog`, which is the index rather than a routed
/// destination. Held to `Chapter::ALL` by
/// `every_declared_chapter_except_the_catalog_is_routed`: without that, a
/// new chapter would leave its commands unreachable through MCP while every
/// assertion here still passed at the old literal count.
const ROUTED_CHAPTERS: &[Chapter] = &[
    Chapter::Data,
    Chapter::Project,
    Chapter::Assets,
    Chapter::GridModel,
    Chapter::PlsCadd,
    Chapter::Survey,
    Chapter::Design,
    Chapter::MapPresentation,
    Chapter::VectorTiles,
    Chapter::Solar,
    Chapter::Reports,
    Chapter::Operations,
    Chapter::Workstation,
];

#[derive(Debug)]
pub struct Surface {
    exposure: Exposure,
    profile: Option<Profile>,
    commands: Vec<Tool>,
    identity: Value,
}

impl Surface {
    pub fn new(
        exposure: Exposure,
        profile: Option<Profile>,
        mut commands: Vec<Tool>,
    ) -> Result<Self, Failure> {
        if profile.is_some() && exposure != Exposure::Commands {
            return Err(Failure::invalid(
                "mcp_profile_exposure_invalid",
                "specialized profiles publish typed command tools and require `--exposure commands`",
            )
            .remedy("pass `--exposure commands --profile <name>`, or omit `--profile`"));
        }
        if let Some(profile) = profile {
            commands.retain(|tool| profile.includes(tool));
            if commands.len() + 2 > profile.tool_limit() {
                return Err(Failure::failed(
                    "mcp_profile_too_broad",
                    format!(
                        "profile `{}` would publish {} tools including `ds_catalog` and `ds_diagnostics`",
                        profile.token(),
                        commands.len() + 2
                    ),
                )
                .remedy("split the profile by operator workflow before publishing it"));
            }
        }
        commands.sort_by(|left, right| left.name.cmp(&right.name));
        Ok(Self {
            exposure,
            profile,
            commands,
            identity: Value::Null,
        })
    }

    pub fn with_identity(mut self, identity: Value) -> Self {
        self.identity = identity;
        self
    }

    pub fn exposure(&self) -> Exposure {
        self.exposure
    }

    pub fn profile(&self) -> Option<Profile> {
        self.profile
    }

    pub fn published_count(&self) -> usize {
        match (self.exposure, self.profile) {
            (Exposure::Chapters, None) => ROUTED_CHAPTERS.len() + 2,
            (Exposure::Commands, Some(_)) => self.commands.len() + 2,
            (Exposure::Commands, None) => self.commands.len() + 1,
            (Exposure::Chapters, Some(_)) => 0,
        }
    }

    pub fn instructions(&self) -> String {
        let instructions = match (self.exposure, self.profile) {
            (Exposure::Chapters, None) => "Use ds_catalog for bounded discovery, call the selected chapter with operation=describe, then operation=invoke. The canonical command descriptor governs arguments, authority, effect, confirmation and refusals; branch on the returned DS envelope.".to_string(),
            (Exposure::Commands, Some(profile)) => format!(
                "This is the typed `{}` profile. Use ds_catalog for bounded discovery, then call the advertised command tool directly. Pass confirm=true only when the command declares it and the user's intent authorizes that exact effect and scope. Branch on the returned DS envelope.",
                profile.token()
            ),
            (Exposure::Commands, None) => "Compatibility command exposure: every advertised tool is one canonical ds command generated from its live descriptor. Pass confirm=true only when declared, branch on the returned DS envelope, and follow typed remedies.".to_string(),
            (Exposure::Chapters, Some(_)) => unreachable!("invalid surface is refused"),
        };
        format!("{instructions} {DEVICE_LINK_GUIDANCE}")
    }

    pub fn tool_list(&self) -> Vec<Value> {
        match (self.exposure, self.profile) {
            (Exposure::Chapters, None) => std::iter::once(catalog_tool_json())
                .chain(std::iter::once(diagnostics_tool_json()))
                .chain(ROUTED_CHAPTERS.iter().copied().map(chapter_tool_json))
                .collect(),
            (Exposure::Commands, Some(_)) => std::iter::once(catalog_tool_json())
                .chain(std::iter::once(diagnostics_tool_json()))
                .chain(self.commands.iter().map(leaf_tool_json))
                .collect(),
            (Exposure::Commands, None) => std::iter::once(diagnostics_tool_json())
                .chain(self.commands.iter().map(leaf_tool_json))
                .collect(),
            (Exposure::Chapters, Some(_)) => Vec::new(),
        }
    }

    pub fn call(
        &self,
        name: &str,
        arguments: &Value,
        executable: &PathBuf,
    ) -> Result<Value, (i64, String)> {
        if name == "ds_diagnostics" {
            return self.call_diagnostics(arguments, executable);
        }
        if name == "ds_catalog" && (self.exposure == Exposure::Chapters || self.profile.is_some()) {
            return self.call_catalog(arguments, executable);
        }
        match self.exposure {
            Exposure::Commands => {
                let Some(tool) = self.commands.iter().find(|tool| tool.name == name) else {
                    return Err((-32602, format!("unknown tool: {name}")));
                };
                invoke_leaf(tool, arguments, executable)
            }
            Exposure::Chapters => {
                let Some(chapter) = ROUTED_CHAPTERS
                    .iter()
                    .copied()
                    .find(|chapter| chapter_tool_name(*chapter) == name)
                else {
                    return Err((-32602, format!("unknown tool: {name}")));
                };
                self.call_chapter(chapter, arguments, executable)
            }
        }
    }

    fn call_chapter(
        &self,
        chapter: Chapter,
        arguments: &Value,
        executable: &PathBuf,
    ) -> Result<Value, (i64, String)> {
        let object = object_with_known_keys(
            arguments,
            &["operation", "command", "arguments", CONFIRM_PROPERTY],
        )?;
        let operation = required_string(&object, "operation")?;
        let command = required_string(&object, "command")?;
        let Some(tool) = self.commands.iter().find(|tool| tool.id == command) else {
            return Err((
                -32602,
                format!("unknown command `{command}`; call `ds_catalog` with a bounded query"),
            ));
        };
        if tool.chapter != chapter {
            return Err((
                -32602,
                format!(
                    "`{command}` belongs to `{}`; call `{}` instead",
                    tool.chapter.token(),
                    chapter_tool_name(tool.chapter)
                ),
            ));
        }
        let nested = object
            .get("arguments")
            .cloned()
            .unwrap_or_else(|| json!({}));
        let confirm = optional_bool(&object, CONFIRM_PROPERTY)?.unwrap_or(false);
        match operation.as_str() {
            "describe" => {
                if confirm || nested.as_object().is_some_and(|values| !values.is_empty()) {
                    return Err((
                        -32602,
                        "`describe` accepts only `operation` and `command`".to_string(),
                    ));
                }
                let argv = vec![
                    "capabilities".to_string(),
                    tool.id.clone(),
                    "--output".to_string(),
                    "json".to_string(),
                ];
                invoke_argv(&argv, executable)
            }
            "invoke" => {
                let mut nested = nested
                    .as_object()
                    .cloned()
                    .ok_or_else(|| (-32602, "`arguments` must be an object".to_string()))?;
                if nested.contains_key(CONFIRM_PROPERTY) {
                    return Err((
                        -32602,
                        format!(
                            "put `{CONFIRM_PROPERTY}` in the chapter envelope, not inside `arguments`"
                        ),
                    ));
                }
                let nested_arguments = Value::Object(nested.clone());
                let confirmation_required = tool
                    .confirmation_required_for(&nested_arguments)
                    .map_err(|message| (-32602, message))?;
                if confirm {
                    if !confirmation_required {
                        return Err((
                            -32602,
                            format!("`{command}` does not accept confirmation for this invocation"),
                        ));
                    }
                    nested.insert(CONFIRM_PROPERTY.to_string(), Value::Bool(true));
                }
                invoke_leaf(tool, &Value::Object(nested), executable)
            }
            _ => Err((
                -32602,
                "`operation` must be `describe` or `invoke`".to_string(),
            )),
        }
    }

    fn call_catalog(
        &self,
        arguments: &Value,
        _executable: &PathBuf,
    ) -> Result<Value, (i64, String)> {
        let object = object_with_known_keys(arguments, &["query", "chapter", "command"])?;
        let query = optional_string(&object, "query")?;
        let chapter = optional_string(&object, "chapter")?;
        let command = optional_string(&object, "command")?;
        if query.is_some() && command.is_some() {
            return Err((
                -32602,
                "pass either `query` or `command`, not both".to_string(),
            ));
        }
        let chapter = chapter
            .map(|token| {
                let parsed = Chapter::from_token(&token).filter(|value| *value != Chapter::Catalog);
                parsed.ok_or_else(|| {
                    (
                        -32602,
                        format!(
                            "unknown routable chapter `{token}`; call `ds_catalog` without filters"
                        ),
                    )
                })
            })
            .transpose()?;
        let visible = self
            .commands
            .iter()
            .filter(|tool| chapter.is_none_or(|value| tool.chapter == value));

        let mut data = if let Some(command) = command {
            let Some(tool) = visible.into_iter().find(|tool| tool.id == command) else {
                if let Some(tool) = self.commands.iter().find(|tool| tool.id == command) {
                    return Err((
                        -32602,
                        format!(
                            "`{command}` belongs to chapter `{}`; describe it with `{}`",
                            tool.chapter.token(),
                            chapter_tool_name(tool.chapter)
                        ),
                    ));
                }
                return Err((-32602, format!("unknown command `{command}`")));
            };
            json!({
                "command": command_summary(tool, &tool.descriptor),
                "next": { "tool": chapter_tool_name(tool.chapter), "arguments": { "operation": "describe", "command": tool.id } },
            })
        } else if let Some(query) = query {
            let terms: Vec<String> = query
                .split_whitespace()
                .map(str::to_lowercase)
                .filter(|term| term.len() > 1)
                .collect();
            if terms.is_empty() {
                return Err((-32602, "`query` needs at least one word".to_string()));
            }
            let mut matches: Vec<(usize, &Tool)> = visible
                .filter_map(|tool| {
                    let haystack = format!("{} {}", tool.id, tool.description).to_lowercase();
                    let score = terms
                        .iter()
                        .filter(|term| haystack.contains(term.as_str()))
                        .count();
                    (score > 0).then_some((score, tool))
                })
                .collect();
            matches.sort_by(|left, right| {
                right
                    .0
                    .cmp(&left.0)
                    .then_with(|| left.1.id.cmp(&right.1.id))
            });
            let matched = matches.len();
            matches.truncate(10);
            let results = matches
                .into_iter()
                .map(|(_, tool)| command_summary(tool, &tool.descriptor))
                .collect::<Vec<_>>();
            json!({
                "query": query,
                "matched": matched,
                "results": results,
                "next": "call the matching chapter with operation=describe and the exact command id",
            })
        } else if let Some(chapter) = chapter {
            let commands = visible
                .map(|tool| command_summary(tool, &tool.descriptor))
                .collect::<Vec<_>>();
            json!({
                "chapter": chapter.token(),
                "tool": chapter_tool_name(chapter),
                "commands": commands,
                "next": { "tool": chapter_tool_name(chapter), "arguments": { "operation": "describe", "command": "<exact-id>" } },
            })
        } else {
            json!({
                "chapters": ROUTED_CHAPTERS.iter().copied().filter_map(|chapter| {
                    let count = self.commands.iter().filter(|tool| tool.chapter == chapter).count();
                    (count > 0).then(|| json!({
                        "chapter": chapter.token(),
                        "tool": chapter_tool_name(chapter),
                        "commands": count,
                        "summary": chapter_description(chapter),
                    }))
                }).collect::<Vec<_>>(),
                "next": "call ds_catalog with one chapter or a bounded query",
            })
        };
        if let Some(object) = data.as_object_mut() {
            object.insert("identity".to_string(), self.identity.clone());
            object.insert(
                "skill_resources".to_string(),
                self.identity.get("skills").cloned().unwrap_or(Value::Null),
            );
        }
        Ok(value_result(data, false))
    }

    fn call_diagnostics(
        &self,
        arguments: &Value,
        executable: &PathBuf,
    ) -> Result<Value, (i64, String)> {
        let object = object_with_known_keys(arguments, &["operation"])?;
        let operation = required_string(&object, "operation")?;
        if operation == "identity" {
            return Ok(value_result(
                json!({
                    "v": 1,
                    "command": "mcp.diagnostics",
                    "contract": 1,
                    "status": "ok",
                    "data": self.identity,
                }),
                false,
            ));
        }
        let argv = match operation.as_str() {
            "doctor" => vec!["doctor", "--output", "json"],
            "shell.status" => vec!["shell", "status", "--output", "json"],
            "capabilities" => vec!["capabilities", "--output", "json"],
            _ => {
                return Err((
                    -32602,
                    "`operation` must be `identity`, `doctor`, `shell.status`, or `capabilities`"
                        .to_string(),
                ));
            }
        }
        .into_iter()
        .map(str::to_string)
        .collect::<Vec<_>>();
        invoke_argv(&argv, executable)
    }
}

fn object_with_known_keys(
    arguments: &Value,
    keys: &[&str],
) -> Result<Map<String, Value>, (i64, String)> {
    let object = match arguments {
        Value::Null => Map::new(),
        Value::Object(object) => object.clone(),
        _ => return Err((-32602, "arguments must be an object".to_string())),
    };
    if let Some(key) = object.keys().find(|key| !keys.contains(&key.as_str())) {
        return Err((-32602, format!("unknown property `{key}`")));
    }
    Ok(object)
}

fn required_string(object: &Map<String, Value>, key: &str) -> Result<String, (i64, String)> {
    optional_string(object, key)?.ok_or_else(|| (-32602, format!("`{key}` is required")))
}

fn optional_string(
    object: &Map<String, Value>,
    key: &str,
) -> Result<Option<String>, (i64, String)> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value)) => Ok(Some(value.clone())),
        Some(_) => Err((-32602, format!("`{key}` must be a string or null"))),
    }
}

fn optional_bool(object: &Map<String, Value>, key: &str) -> Result<Option<bool>, (i64, String)> {
    match object.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        Some(_) => Err((-32602, format!("`{key}` must be a boolean"))),
    }
}

fn command_summary(tool: &Tool, descriptor: &Value) -> Value {
    json!({
        "id": tool.id,
        "chapter": tool.chapter.token(),
        "summary": descriptor["summary"],
        "availability": descriptor["availability"],
        "next": { "tool": chapter_tool_name(tool.chapter), "operation": "describe" },
    })
}

fn invoke_leaf(
    tool: &Tool,
    arguments: &Value,
    executable: &PathBuf,
) -> Result<Value, (i64, String)> {
    let confirmation_required = tool
        .confirmation_required_for(arguments)
        .map_err(|message| (-32602, message))?;
    let argv = tools::argv_for_call(tool, arguments).map_err(|message| (-32602, message))?;
    // The registry owns confirmation and refuses before any handler opens a
    // bridge. Preserve that ordering here too: an unconfirmed paired write is
    // an input refusal, never a reason to start a desktop.
    if confirmation_required
        && !arguments
            .get(tools::CONFIRM_PROPERTY)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return invoke_argv(&argv, executable);
    }
    if let Err(failure) = tools::ensure_desktop(tool, arguments, executable) {
        return Ok(failure_result(tool, &failure));
    }
    invoke_argv(&argv, executable)
}

// The CLI retains its trusted-terminal login contract. MCP cannot collect
// credentials, so translate only advice that would send an MCP caller there.
// A password-only command stays an explicit coverage gap instead of pretending
// that device linking will make it work.
fn mcp_device_link_guidance(value: &mut Value) {
    match value {
        Value::Object(fields) => {
            let password_only = fields
                .get("message")
                .or_else(|| fields.get("when"))
                .and_then(Value::as_str)
                .is_some_and(|message| {
                    message.contains("needs the password session")
                        || message.contains("requires a password session")
                });
            for (name, child) in fields.iter_mut() {
                if matches!(name.as_str(), "remedy" | "next")
                    && child
                        .as_str()
                        .is_some_and(|advice| advice.contains("auth login"))
                {
                    *child = Value::String(
                        match (name.as_str(), password_only) {
                            ("next", true) => "ds auth status",
                            ("next", false) => "auth.link.begin",
                            (_, true) => PASSWORD_ONLY_REMEDY,
                            _ => DEVICE_LINK_REMEDY,
                        }
                        .to_string(),
                    );
                } else {
                    mcp_device_link_guidance(child);
                }
            }
        }
        Value::Array(items) => {
            for item in items {
                mcp_device_link_guidance(item);
            }
        }
        _ => {}
    }
}

fn failure_result(tool: &Tool, failure: &Failure) -> Value {
    let contract = tool.descriptor["contract"].as_u64().unwrap_or(1) as u32;
    let mut envelope = serde_json::to_value(error_envelope(&tool.id, contract, failure))
        .unwrap_or_else(|_| json!({ "status": "error" }));
    mcp_device_link_guidance(&mut envelope);
    let text = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": envelope,
        "isError": true,
    })
}

fn invoke_argv(argv: &[String], executable: &PathBuf) -> Result<Value, (i64, String)> {
    let (code, stdout, stderr) =
        tools::run_cli(executable, argv).map_err(|message| (-32000, message))?;
    let mut envelope: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    let is_error = code != 0
        || envelope
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            != Some("ok");
    if let Some(envelope) = &mut envelope {
        mcp_device_link_guidance(envelope);
    }
    let text = envelope.as_ref().map_or_else(
        || {
            if stdout.trim().is_empty() {
                stderr.trim().to_string()
            } else {
                stdout.trim().to_string()
            }
        },
        |value| serde_json::to_string(value).unwrap_or_else(|_| "{}".to_string()),
    );
    let mut result = json!({
        "content": [{ "type": "text", "text": text }],
        "isError": is_error,
    });
    if let Some(envelope) = envelope {
        result["structuredContent"] = envelope;
    }
    Ok(result)
}

fn value_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": is_error,
    })
}

pub fn leaf_tool_json(tool: &Tool) -> Value {
    json!({
        "name": tool.name,
        "title": tool.id,
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "annotations": {
            "title": tool.id,
            "readOnlyHint": matches!(
                tool.descriptor.get("effect").and_then(Value::as_str),
                Some("discovery" | "read_only" | "proposal")
            ),
            "openWorldHint": false,
        },
    })
}

/// The chapters `ds_catalog` will accept, read from the routing table rather
/// than written out again.
///
/// It WAS written out again, and it had drifted: `data` was missing, so an
/// agent that found `desktop.data.rwanda.install` through a query could not
/// then ask for that chapter — the one chapter whose whole job is putting the
/// country's data on the machine was the one it could not name.
fn catalog_chapter_enum() -> Value {
    let mut tokens: Vec<Value> = ROUTED_CHAPTERS
        .iter()
        .map(|chapter| json!(chapter.token()))
        .collect();
    tokens.push(Value::Null);
    Value::Array(tokens)
}

fn catalog_tool_json() -> Value {
    json!({
        "name": "ds_catalog",
        "title": "DS catalogue",
        "description": "Discover DS chapters, search bounded command summaries, and route one exact command to its live descriptor. Returns no bulk schemas.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "query": { "type": ["string", "null"], "description": "Words to match against command ids and descriptions; at most ten summaries return." },
                "chapter": { "type": ["string", "null"], "enum": catalog_chapter_enum(), "description": "Restrict discovery to one operator-intent chapter." },
                "command": { "type": ["string", "null"], "description": "Route one exact canonical command id to its chapter describe call." }
            },
            "additionalProperties": false
        },
        "annotations": { "title": "DS catalogue", "readOnlyHint": true, "openWorldHint": false }
    })
}

fn diagnostics_tool_json() -> Value {
    json!({
        "name": "ds_diagnostics",
        "title": "DS diagnostics",
        "description": "Obtain bounded bootstrap identity or invoke the same read-only version/doctor/shell/capabilities implementations as the CLI. Requires no map, project, desktop session, or confirmation.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "operation": {
                    "type": "string",
                    "enum": ["identity", "doctor", "shell.status", "capabilities"],
                    "description": "Select one bounded read-only diagnostic."
                }
            },
            "required": ["operation"],
            "additionalProperties": false
        },
        "annotations": {
            "title": "DS diagnostics",
            "readOnlyHint": true,
            "destructiveHint": false,
            "idempotentHint": true,
            "openWorldHint": false
        }
    })
}

fn chapter_tool_json(chapter: Chapter) -> Value {
    let name = chapter_tool_name(chapter);
    json!({
        "name": name,
        "title": chapter.token(),
        "description": chapter_description(chapter),
        "inputSchema": {
            "type": "object",
            "properties": {
                "operation": { "type": "string", "enum": ["describe", "invoke"], "description": "Describe the live command contract before invoking it." },
                "command": { "type": "string", "description": "Exact canonical ds command id in this chapter." },
                "arguments": { "type": "object", "description": "Command arguments validated against the live descriptor before dispatch." },
                "confirm": { "type": "boolean", "default": false, "description": "Maps to --yes only when this exact command contract requires confirmation and user intent authorizes it." }
            },
            "required": ["operation", "command"],
            "additionalProperties": false
        },
        "annotations": { "title": chapter.token(), "readOnlyHint": false, "openWorldHint": false }
    })
}

pub const fn chapter_tool_name(chapter: Chapter) -> &'static str {
    match chapter {
        Chapter::Catalog => "ds_catalog",
        Chapter::Data => "ds_data",
        Chapter::Project => "ds_project",
        Chapter::Assets => "ds_assets",
        Chapter::GridModel => "ds_grid_model",
        Chapter::PlsCadd => "ds_pls_cadd",
        Chapter::Survey => "ds_survey",
        Chapter::Design => "ds_design",
        Chapter::MapPresentation => "ds_map_presentation",
        Chapter::VectorTiles => "ds_vector_tiles",
        Chapter::Solar => "ds_solar",
        Chapter::Reports => "ds_reports",
        Chapter::Operations => "ds_operations",
        Chapter::Workstation => "ds_workstation",
    }
}

pub const fn chapter_description(chapter: Chapter) -> &'static str {
    match chapter {
        Chapter::Catalog => "Discover DS chapters, commands, and one exact live contract.",
        Chapter::Data => {
            "Prepare local data for analysis, and hold the country's geographic reference datasets on this computer. Inspect a source file, then convert it to the analytical GeoParquet format; conversion is an explicit step that runs before analysis, never inside it, and needs no project or paired desktop. Downloading and indexing datasets is routine and reversible: it spends disk and nothing else, changes nothing in the cloud, is undone by `desktop data rwanda remove`, and is safe to run on the way to a report, a map or any other action without asking anyone first. Describe a command before invoking it."
        }
        Chapter::Project => {
            "Establish project context and manage project plans, tasks, assignments, and records. Describe a command before invoking it."
        }
        Chapter::Assets => {
            "Browse, preview, classify, promote, link and ingest the documents a project holds, in declared and auto-indexed folders. Describe a command before invoking it."
        }
        Chapter::GridModel => {
            "Inspect, validate, project, revise, import, and export canonical grid models. Describe a command before invoking it."
        }
        Chapter::PlsCadd => {
            "Work with native PLS-CADD deliveries and pinned engineering libraries: inspect capacity and references, reconcile terrain, label deviations, verify delivery, and resolve exact native assets. Describe a command before invoking it."
        }
        Chapter::Survey => {
            "Manage Form Factory schemas, project-form settings, project templates and project creation without map state; or work with survey/map-owned local data. Describe a command before invoking it."
        }
        Chapter::Design => {
            "Read, stage, process, report, save, or discard transformer and LV design work. Describe a command before invoking it."
        }
        Chapter::MapPresentation => {
            "Read or change project map styling and its secondary visual dimension. Describe a command before invoking it."
        }
        Chapter::VectorTiles => {
            "Inspect and manage project vector-tile outputs: status, source preflight, generation planning, confirmed generation, and catalogue membership. Describe a command before invoking it."
        }
        Chapter::Solar => {
            "Prepare, run, inspect, publish, and export Solar work. Describe a command before invoking it."
        }
        Chapter::Reports => {
            "Discover report tasks, export or bundle verified report artifacts, and produce the selected project's compounded deliverable in the background. Describe a command before invoking it."
        }
        Chapter::Operations => {
            "Inspect platform health, manage shell reachability, and report product gaps. Describe a command before invoking it."
        }
        Chapter::Workstation => {
            "Inspect workstation prerequisites and governed reference components, review mutation-free plans, verify local evidence, and read or clean what this machine holds for a project. Describe a command before invoking it."
        }
    }
}

// One bounded end-to-end printing workflow, on either host. The headless
// loop is the production route: setups are read and published natively
// (`report.layout.*`), the project's context is held and seeded on this
// machine (`data.project-cache.*`), the output selection is saved
// (`report.project.outputs.set`) and every sheet renders with that context
// (`report.project.export`, `--seed` on the first print). The paired leaves
// remain for what has no headless owner yet: survey and local-layer context,
// per-transformer overrides, district and custom-area maps.
const PRINTING_COMMANDS: &[&str] = &[
    "assets.map.publish",
    "report.layout.render",
    "assets.maps",
    "map.print.schema",
    "report.layout.context",
    "report.layout.list",
    "report.layout.get",
    "report.layout.edit",
    "report.layout.save",
    "report.transformers",
    "report.plan",
    "data.project-cache.status",
    "data.project-cache.seed",
    "data.city-vectors",
    "report.project.settings",
    "report.project.outputs.set",
    "report.project.export",
    "report.project.map-inputs",
    "map.design.attach-print",
    "desktop.printing.settings",
    "desktop.printing.prepare",
    "desktop.printing.transformers",
    "desktop.printing.export",
    "desktop.printing.artifact.read",
    "desktop.printing.seed-context",
    "desktop.printing.map.export",
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mcp_advertises_device_link_for_every_exposure() {
        for (exposure, profile) in [
            (Exposure::Chapters, None),
            (Exposure::Commands, None),
            (Exposure::Commands, Some(Profile::AuthContext)),
        ] {
            let surface = Surface::new(exposure, profile, vec![]).expect("surface");
            let instructions = surface.instructions();
            assert!(instructions.contains("auth.link.begin"));
            assert!(instructions.contains("auth.link.complete"));
            assert!(!instructions.contains("auth login"));
        }
    }

    #[test]
    fn mcp_signed_out_guidance_replaces_password_advice_in_both_shapes() {
        let mut descriptor = json!({
            "data": {"command": {"refusals": [{
                "code": "headless_signed_out",
                "remedy": "run ds auth login --email <address>"
            }]}}
        });
        mcp_device_link_guidance(&mut descriptor);
        assert_eq!(
            descriptor["data"]["command"]["refusals"][0]["remedy"],
            DEVICE_LINK_REMEDY
        );

        let mut error = json!({
            "error": {
                "code": "headless_signed_out",
                "message": "no native user is signed in",
                "remedy": "run ds auth login --email <address>",
                "next": "ds auth login --email <address>"
            }
        });
        mcp_device_link_guidance(&mut error);
        assert_eq!(error["error"]["remedy"], DEVICE_LINK_REMEDY);
        assert_eq!(error["error"]["next"], "auth.link.begin");
        assert!(!error.to_string().contains("auth login"));
    }

    #[test]
    fn mcp_password_only_gap_is_truthful_and_unrelated_remedies_survive() {
        let mut error = json!({
            "error": {
                "code": "headless_signed_out",
                "message": "this command needs the password session",
                "remedy": "run ds auth login --email <address>",
                "next": "ds auth status"
            },
            "unrelated": {"remedy": "repair the package"}
        });
        mcp_device_link_guidance(&mut error);
        assert_eq!(error["error"]["remedy"], PASSWORD_ONLY_REMEDY);
        assert_eq!(error["error"]["next"], "ds auth status");
        assert_eq!(error["unrelated"]["remedy"], "repair the package");
    }
    use ds_cli_contract::spec::Authority;

    fn tool(id: &str, chapter: Chapter, confirmation_required: bool) -> Tool {
        Tool {
            name: tools::tool_name(id),
            id: id.to_string(),
            chapter,
            authority: Authority::None,
            path: id.split('.').map(str::to_string).collect(),
            description: format!("{id} purpose"),
            input_schema: json!({ "type": "object" }),
            confirmation_required,
            confirmation_trigger: None,
            inputs: Vec::new(),
            descriptor: json!({
                "id": id,
                "summary": format!("{id} summary"),
                "availability": "available",
                "effect": if confirmation_required { "global_write" } else { "read_only" }
            }),
        }
    }

    fn conditional_tool(id: &str, chapter: Chapter) -> Tool {
        let mut tool = tool(id, chapter, true);
        tool.descriptor["effect"] = json!("machine_write");
        tool.confirmation_trigger = Some("write".to_string());
        tool.inputs.push(tools::Input {
            name: "write".to_string(),
            kind: "switch".to_string(),
        });
        tool
    }

    #[test]
    fn broad_surface_is_exactly_the_declared_stable_chapter_tools() {
        let surface = Surface::new(
            Exposure::Chapters,
            None,
            vec![tool("pls.reference-closure", Chapter::PlsCadd, false)],
        )
        .expect("surface");
        let names = surface
            .tool_list()
            .into_iter()
            .map(|value| value["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        // Derived: the catalogue and diagnostics bootstrap plus one router
        // per routed chapter. A new chapter that nobody routed fails here.
        assert_eq!(names.len(), Chapter::ALL.len() + 1);
        assert_eq!(names[0], "ds_catalog");
        for chapter in Chapter::ALL {
            assert!(
                names.contains(&chapter_tool_name(*chapter).to_string()),
                "chapter `{chapter}` publishes no tool"
            );
        }
    }

    #[test]
    fn every_declared_chapter_except_the_catalog_is_routed() {
        // F36: `ROUTED_CHAPTERS` is hand-maintained beside a declaration that
        // already enumerates every chapter. Adding a chapter and forgetting
        // this list makes its commands unreachable through MCP with nothing
        // failing, because the surface's own count matches the list, not the
        // registry.
        let expected: Vec<Chapter> = Chapter::ALL
            .iter()
            .copied()
            .filter(|chapter| *chapter != Chapter::Catalog)
            .collect();
        assert_eq!(
            ROUTED_CHAPTERS.to_vec(),
            expected,
            "`ROUTED_CHAPTERS` must be `Chapter::ALL` minus the catalogue, in declaration order"
        );
    }

    #[test]
    fn profiles_are_typed_filtered_views_and_require_command_exposure() {
        let tools = vec![
            tool("pls.reference-closure", Chapter::PlsCadd, false),
            tool("library.resolve-native", Chapter::PlsCadd, false),
            tool("tile.generate", Chapter::VectorTiles, true),
        ];
        let profile =
            Surface::new(Exposure::Commands, Some(Profile::Pls), tools.clone()).expect("profile");
        let names = profile
            .tool_list()
            .into_iter()
            .map(|value| value["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["ds_catalog", "ds_diagnostics", "pls_reference-closure"]
        );
        let library = Surface::new(Exposure::Commands, Some(Profile::PlsLibrary), tools.clone())
            .expect("library profile");
        let names = library
            .tool_list()
            .into_iter()
            .map(|value| value["name"].as_str().unwrap().to_string())
            .collect::<Vec<_>>();
        assert_eq!(
            names,
            ["ds_catalog", "ds_diagnostics", "library_resolve-native"]
        );
        let error = Surface::new(Exposure::Chapters, Some(Profile::Pls), tools).unwrap_err();
        assert_eq!(error.code(), "mcp_profile_exposure_invalid");
    }

    #[test]
    fn wrong_chapter_and_nested_confirmation_fail_closed() {
        let surface = Surface::new(
            Exposure::Chapters,
            None,
            vec![tool("tile.generate", Chapter::VectorTiles, true)],
        )
        .expect("surface");
        let executable = PathBuf::from("not-called");
        let wrong = surface
            .call_chapter(
                Chapter::Survey,
                &json!({ "operation": "invoke", "command": "tile.generate", "arguments": {} }),
                &executable,
            )
            .unwrap_err();
        assert!(wrong.1.contains("ds_vector_tiles"), "{}", wrong.1);
        let nested = surface
            .call_chapter(
                Chapter::VectorTiles,
                &json!({ "operation": "invoke", "command": "tile.generate", "arguments": { "confirm": true } }),
                &executable,
            )
            .unwrap_err();
        assert!(nested.1.contains("chapter envelope"), "{}", nested.1);
    }

    #[test]
    fn conditional_tool_annotations_stay_conservative_and_preview_rejects_confirm() {
        let conditional = conditional_tool("operations.install", Chapter::Operations);
        assert_eq!(
            leaf_tool_json(&conditional)["annotations"]["readOnlyHint"],
            false
        );
        let surface = Surface::new(Exposure::Chapters, None, vec![conditional]).unwrap();
        let error = surface
            .call_chapter(
                Chapter::Operations,
                &json!({
                    "operation": "invoke",
                    "command": "operations.install",
                    "arguments": { "write": false },
                    "confirm": true,
                }),
                &PathBuf::from("not-called"),
            )
            .unwrap_err();
        assert!(
            error
                .1
                .contains("does not accept confirmation for this invocation"),
            "{}",
            error.1
        );
    }

    #[test]
    fn non_confirming_local_ui_tools_are_not_annotated_read_only() {
        let mut local_ui = tool("data.admin-bounds.read", Chapter::Data, false);
        local_ui.descriptor["effect"] = json!("local_ui");
        assert_eq!(
            leaf_tool_json(&local_ui)["annotations"]["readOnlyHint"],
            false
        );
    }

    #[test]
    fn catalog_discovery_never_enters_the_desktop_launch_gate() {
        let mut paired = tool("desktop.project.list", Chapter::Project, false);
        paired.authority = Authority::DesktopUser;
        let surface = Surface::new(Exposure::Chapters, None, vec![paired]).expect("surface");
        let response = surface
            .call_catalog(
                &json!({ "command": "desktop.project.list" }),
                &PathBuf::from("ds"),
            )
            .expect("catalogue is descriptor-only");
        assert_eq!(
            response["structuredContent"]["command"]["id"],
            "desktop.project.list"
        );
    }

    #[test]
    fn every_declared_profile_token_round_trips() {
        for token in PROFILE_IDS {
            let profile = Profile::from_token(token).expect("known profile");
            assert_eq!(profile.token(), *token);
        }
        assert!(Profile::from_token("all").is_none());
    }

    #[test]
    fn bundled_skills_name_only_known_chapters_and_compatible_profiles() {
        let skills = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../skills");
        let mut declared = 0usize;
        for entry in std::fs::read_dir(skills).expect("skills directory") {
            let path = entry.expect("skill entry").path().join("SKILL.md");
            if !path.is_file() {
                continue;
            }
            let text = std::fs::read_to_string(&path).expect("skill text");
            let chapters = text.lines().find_map(|line| {
                line.trim_start().strip_prefix("ds-chapters:").map(|value| {
                    value
                        .split(',')
                        .map(str::trim)
                        .map(|token| {
                            Chapter::from_token(token).unwrap_or_else(|| {
                                panic!("{} names unknown chapter `{token}`", path.display())
                            })
                        })
                        .collect::<Vec<_>>()
                })
            });
            let profile = text.lines().find_map(|line| {
                line.trim_start()
                    .strip_prefix("ds-mcp-profile:")
                    .map(|value| {
                        let token = value.trim();
                        Profile::from_token(token).unwrap_or_else(|| {
                            panic!("{} names unknown profile `{token}`", path.display())
                        })
                    })
            });
            if let Some(chapters) = chapters {
                declared += 1;
                assert!(!chapters.is_empty(), "{} has no chapters", path.display());
                if let Some(profile) = profile {
                    for chapter in chapters {
                        assert!(
                            profile.includes_chapter(chapter),
                            "{} requires `{}` but profile `{}` omits it",
                            path.display(),
                            chapter.token(),
                            profile.token()
                        );
                    }
                }
            } else {
                assert!(
                    profile.is_none(),
                    "{} names a profile without declaring chapters",
                    path.display()
                );
            }
        }
        assert!(declared >= 10, "workflow skills must declare chapters");
    }
}
