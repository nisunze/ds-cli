//! MCP publication shapes generated from the live command descriptors.

use std::path::PathBuf;
use std::time::Duration;

use ds_cli_contract::outcome::{Failure, error_envelope};
use ds_cli_contract::spec::{Chapter, Effect};
use serde_json::{Map, Value, json};

use crate::tools::{self, CONFIRM_PROPERTY, Tool};

/// The bound on one tool call's `ds` child unless `--call-timeout` says
/// otherwise: an hour, so long engine work finishes and only a hung child is
/// stopped.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(3600);

/// The largest envelope, serialized, that one tool result carries. Every
/// command already bounds its own answer; this is the adapter's backstop for
/// a host context, and an envelope above it is trimmed — never cut — and says
/// so in `more.mcp_truncation`.
pub const MAX_RESULT_BYTES: usize = 256 * 1024;

/// Object keys whose string value is a credential wherever it appears in an
/// answer. No command emits one — each owner holds that in its own tests —
/// and this adapter does not rely on it: a value under one of these keys is
/// replaced before any host can read it. `token` is absent on purpose: it is
/// ordinary data in this product (a survey feature-code token, a host token).
const CREDENTIAL_KEYS: &[&str] = &[
    "password",
    "access_token",
    "refresh_token",
    "id_token",
    "client_secret",
    "session_secret",
    "private_key",
    "device_code",
    "code_verifier",
];

pub const REDACTED: &str = "[redacted by ds mcp]";

/// Non-envelope text — a child that printed no JSON — is diagnostic only,
/// and bounded to this many bytes.
const MAX_FALLBACK_TEXT_BYTES: usize = 4 * 1024;

pub const EXPOSURES: &[&str] = &["chapters", "commands"];

// The only sign-in this surface ever advertises is the device link a person
// approves in their signed-in Desktop. One sentence, on every exposure: a
// person who has never opened a terminal can follow it, and it is the same
// sentence `ds` itself gives. Nothing here names a terminal sign-in, an
// address or a secret; `mcp_device_link_guidance` below scrubs any that
// slips through from a descriptor, and `crates/ds/tests/mcp.rs` proves the
// published surface carries none.
const DEVICE_LINK_GUIDANCE: &str = "If signed out, run `ds account connect` on this machine (or call the account.connect tool), then approve the request in the signed-in DS GridDesign Desktop under Account > Link a trusted device; call account.connect again once approved. Keep the same lane throughout. That is the only sign-in.";
const PROJECT_WORK_GUIDANCE: &str = "For substantive delivery on a named DS project, read `ds-project-work` from the receipt-verified skill resources. Use Project Work as the delivery record: as the signed-in user and for the exact project, inspect `pm.plan` and existing `pm.task` items, reuse a matching task or open one, assign only through the declared project-member workflow, and record observed progress, review and verified completion. Trace real correspondence, design comments, submissions and exact revisions through their declared commands; verify asset links before claiming an attachment. If delivery is blocked, record its real cause and next condition in the task; `pm.task.block` is only for an answer owed on a record. Server-owned notifications follow applicable committed PM writes; an assignment receipt is not proof that a notification reached anyone. Do not create tasks for casual reads or claim to act for another user.";
// Product-use reporting duty (owner, 2026-10-07; ds-command-kernel
// `docs/contracts/ds-cli-reliability-and-feedback.md` §5): a sighting is filed
// when it happens, never only when someone asks; coding sessions use their
// owning backlogs instead.
const FEEDBACK_GUIDANCE: &str = "Report as you go: outside coding sessions, file a feedback.submit sighting (feedback.draft --last drafts it from the last failure) as soon as it happens, unasked, when a command failed, crashed or refused past its remedy; finding it took a second search or the user's words missed it; help, refusal or skill text disagreed with what it did; output was wrong, incomplete or surprising; a step was slow or needed a workaround; a capability was missing; or an idea would save the user steps. One sighting per distinct gap with expected behavior, evidence and acceptance, never secrets or customer data; repeats merge into occurrences. Ask no permission, and list the report ids at the end of the task. Coding sessions never submit feedback; they fix findings or record unfinished work in the owning backlogs.";
const REGRESSION_GUIDANCE: &str = "For reported regressions, replay the exact command against installed `ds` and a negative control, compare the last working result, then distinguish a code defect, unclear CLI/MCP guidance, a missing authorized capability, and misuse of a clear contract. Fix or clarify only the demonstrated gap; note or close official feedback with evidence and remove resolved work from `ds-command-kernel/backlog/OPEN.md` when that checkout is available.";
const BOOKLET_GUIDANCE: &str = "For MV plan/profile booklets, read the receipt-verified `ds-printout` skill and its booklet reference before rendering. Use the governed A3 landscape cover and naming template at `ds-work/standards/mv-plan-profile/frontmatter`, with exact project-owned parties, contract, logos, model, version and date. Use canonical structure examples only with proved member mapping; otherwise use exact placed native names without an equivalence claim. Omit unknown facts and keep unresolved findings in the internal receipt. If the template or project facts are inaccessible through the declared DS surface, report the gap and stop the booklet job; do not redraw a cover or borrow another project's branding. Assemble cover, naming, project key map, then numbered sheets, preserving A3 landscape and `DS GridDesign by datasolutions.rw` on every page. Inspect a complete representative PDF at paper size and verify online bytes before claiming delivery. If the live `ds` contract cannot perform an atomic booklet job, report that product gap; local assembly is not a governed project publication.";
/// Word for word `ds_cli_auth::SIGNED_OUT_REMEDY`. Spelled here because this
/// crate reaches `ds` only through its executable, never its crates;
/// `crates/ds/tests/mcp.rs` holds the two equal.
pub const DEVICE_LINK_REMEDY: &str = "run `ds account connect`";
pub const DEVICE_LINK_NEXT: &str = "account.connect";
/// Words no MCP answer may carry: each is the beginning of advice that sends
/// a person to a terminal sign-in. Matched case-insensitively.
pub const TERMINAL_SIGN_IN_WORDS: &[&str] = &["auth login", "password"];
pub const PROFILE_IDS: &[&str] = &[
    "auth-context",
    "admin-bounds",
    "datasets",
    "grid",
    "grid-native",
    "grid-corrections",
    "printing",
    "printing-maps",
    "grid-local-model",
    "clearance",
    "survey",
    "form-factory",
    "survey-projects",
    "survey-media",
    "survey-migration",
    "design-edit",
    "design-run",
    "map",
    "grid-profile",
    "styles",
    "print-styles",
    "layers",
    "tiling",
    "project",
    "project-task-operations",
    "project-checklists",
    "notes",
    "correspondence",
    "messaging",
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
    Datasets,
    Installations,
    Grid,
    GridNative,
    GridCorrections,
    Printing,
    PrintingMaps,
    GridLocalModel,
    GridClearance,
    Survey,
    FormFactory,
    SurveyProjects,
    SurveyMedia,
    SurveyMigration,
    DesignEdit,
    DesignRun,
    Map,
    GridProfile,
    Styles,
    PrintStyles,
    Layers,
    Tiling,
    Project,
    ProjectTaskOperations,
    ProjectChecklists,
    Notes,
    Correspondence,
    Messaging,
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
            "datasets" => Some(Self::Datasets),
            "installations" => Some(Self::Installations),
            "grid" => Some(Self::Grid),
            "grid-native" => Some(Self::GridNative),
            "grid-corrections" => Some(Self::GridCorrections),
            "printing" => Some(Self::Printing),
            "printing-maps" => Some(Self::PrintingMaps),
            "grid-local-model" => Some(Self::GridLocalModel),
            "clearance" => Some(Self::GridClearance),
            "survey" => Some(Self::Survey),
            "form-factory" => Some(Self::FormFactory),
            "survey-projects" => Some(Self::SurveyProjects),
            "survey-media" => Some(Self::SurveyMedia),
            "survey-migration" => Some(Self::SurveyMigration),
            "design-edit" => Some(Self::DesignEdit),
            "design-run" => Some(Self::DesignRun),
            "map" => Some(Self::Map),
            "grid-profile" => Some(Self::GridProfile),
            "styles" => Some(Self::Styles),
            "print-styles" => Some(Self::PrintStyles),
            "layers" => Some(Self::Layers),
            "tiling" => Some(Self::Tiling),
            "project" => Some(Self::Project),
            "project-task-operations" => Some(Self::ProjectTaskOperations),
            "project-checklists" => Some(Self::ProjectChecklists),
            "notes" => Some(Self::Notes),
            "correspondence" => Some(Self::Correspondence),
            "messaging" => Some(Self::Messaging),
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
            Self::Datasets => "datasets",
            Self::Installations => "installations",
            Self::Grid => "grid",
            Self::GridNative => "grid-native",
            Self::GridCorrections => "grid-corrections",
            Self::Printing => "printing",
            Self::PrintingMaps => "printing-maps",
            Self::GridLocalModel => "grid-local-model",
            Self::GridClearance => "clearance",
            Self::Survey => "survey",
            Self::FormFactory => "form-factory",
            Self::SurveyProjects => "survey-projects",
            Self::SurveyMedia => "survey-media",
            Self::SurveyMigration => "survey-migration",
            Self::DesignEdit => "design-edit",
            Self::DesignRun => "design-run",
            Self::Map => "map",
            Self::GridProfile => "grid-profile",
            Self::Styles => "styles",
            Self::PrintStyles => "print-styles",
            Self::Layers => "layers",
            Self::Tiling => "tiling",
            Self::Project => "project",
            Self::ProjectTaskOperations => "project-task-operations",
            Self::ProjectChecklists => "project-checklists",
            Self::Notes => "notes",
            Self::Correspondence => "correspondence",
            Self::Messaging => "messaging",
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
            // Reviewed batch corrections and native sync now have separate
            // routes, keeping this broad typed profile within its budget.
            // Raised to 29 on 2026-09-25 for `dsgrid replace-structure`, the
            // file-in/file-out sibling of `import-structure` that sits beside
            // it: a router that can add a definition but not replace one
            // leaves raised allowable tables to post-export patches.
            // Raised to 31 on 2026-09-27 for `dsgrid spotting
            // preview-receipt`: `ds-grid-spotting` names this profile, and
            // a router that plans whole-model spotting but cannot show a
            // receipt whose rejected rows were truncated leaves that
            // proposal unviewable.
            // Thirty report/model leaves plus both bootstrap tools. The
            // installed canary already publishes this exact 32-tool set:
            // report.engine is the reporter identity needed to prove a
            // local export, while the new deprecated ZIP alias is excluded.
            // Exact retained-source cable reconciliation adds one file-authoring leaf.
            // 2026-10-09: `dsgrid spotting apply-receipt` lands a complete
            // whole-model spotting receipt beside the preview of a truncated
            // one; without it a valid proposal whose diagnostics exceed the
            // 16 MiB params bound had no apply path in this profile.
            Self::Grid => 34,
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
            // Single-target finite member grants add read/plan/apply.
            // Cascade preview and native delete complete the existing entry
            // workflow without displacing project bindings or template tools.
            // Raised to 28 on 2026-10-19 for `survey.entries.update-plan` and
            // `survey.entries.update` (34f36747): the version-fenced correction
            // is the same governed entry lifecycle as create and delete, so
            // splitting it out would put one entry's lifecycle in two profiles.
            Self::SurveyProjects => 28,
            // Twenty-one governed design-edit leaves plus the two bootstrap
            // tools. Version history and the pinned Working set project the
            // same bounded desktop-owned workflow without transporting
            // features through MCP.
            Self::DesignEdit => 23,
            // Twenty-four print production leaves plus both bootstrap tools:
            // city vector acquisition adds one step to the existing headless
            // printing workflow, including projects without held context. The
            // headless loop (context held and seeded on this machine, output
            // selection, export with context) beside the paired leaves that
            // still need the desktop's own holdings. Map composition and
            // publication are a separate focused workflow.
            // Named printing additions: report.project.crs captures the map's
            // authority; report.voltage-drop-combined assembles saved status
            // pages. Both stay in this workflow, outside the bounded Grid router.
            Self::Printing => 29,
            Self::PrintingMaps => 7,
            // The layer drawer's profile also carries this machine's prepared
            // local layer catalogue: seventeen leaves plus both bootstrap tools.
            // Preparing, renaming and removing a local layer is the same
            // operator workflow as the local tile references beside it — one
            // host's own layers — so it is not a second profile.
            // Raised to 21 on 2026-10-19 by the retained overlay registry read
            // and its local edit: the same machine-local layer workflow.
            Self::Layers => 21,
            // Seventeen leaves plus both bootstrap tools. The one that raised
            // this from eighteen on 2026-10-09 is `ds feedback draft --last`:
            // every failed envelope names it as its next command, so a profile
            // that can submit a report but not draft one from the failure it
            // just saw would send the caller to a tool it does not publish.
            // The one that raised this from seventeen on 2026-09-18 is
            // `ds feedback note`: with
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
            Self::Operations => 19,
            // Fourteen background-report leaves plus two bootstrap tools.
            // Project output settings and canonical MV setup live in the
            // focused Printing profile. Raised from the
            // default on 2026-09-19 by the publication queue: an agent doing
            // background delivery work produces report artifacts, and a
            // profile that can produce them but cannot publish them or read
            // where they are strands its own output on the machine. Lowered
            // on 2026-10-22 when the cloud `report.project.compute` was
            // deleted: reports compute only on an edge (`report.project.export`).
            Self::ProjectOperations => 17,
            // Twenty-four Project Work leaves plus both bootstrap tools.
            // The task-geometry reads/writes keep a comment's WHERE attached
            // to its plan item. Task delete and the three deletion-backup
            // leaves now complete one reviewed delete/recover workflow: a
            // caller can inspect the plan, delete at its exact revision,
            // inspect the durable backup, and restore at a fresh revision.
            // One bounded task/milestone/record/note overview joins the existing leaves.
            // Actual discovery is now 27 tools including both bootstrap tools.
            Self::Project => 27,
            // Twelve private-note and ten shared project-note leaves plus both
            // bootstrap tools: one note-keeping workflow split from `project`.
            Self::Notes => 24,
            Self::Messaging => 14,
            // Task creation with typed geometry, geometry edits, and
            // admission/field-hours steps: nine leaves plus two bootstrap tools.
            // Raised to 14 on 2026-10-02 by the three model-link leaves: the
            // exact DS Grid model version a work item is about sits beside its
            // WHERE, and reading which work links a model is the same lane.
            // Thirteen task leaves plus two bootstrap tools; detach retains files.
            Self::ProjectTaskOperations => 15,
            // Seventeen working-copy leaves plus both bootstrap tools. Raised
            // from the default on 2026-09-22 when the four 2026-09-21 leaves
            // (`dsgrid model forget`, `dsgrid structure admin-refresh`,
            // `dsgrid profile labels set|show`) were routed here from the
            // broad `grid` router they had pushed past its own budget. Raised
            // to 22 on 2026-09-24 for the governed head's versions, retire and
            // restore, routed here for the same reason (e62edf85).
            // Exact project-model GeoJSON export is the read half of the
            // governed head workflow and keeps its source revision attached.
            // The project list is the bounded entry to each model's separate
            // version history; without it the profile cannot choose an ID.
            // Raised on 2026-09-25 for the six package-asset leaves (`dsgrid asset
            // list|extract|attach|detach`, `dsgrid project asset
            // list|extract`): a version's original PLS-CADD upload and its
            // delivered backup belong with the version lifecycle, and the
            // broad `grid` router cannot absorb six more.
            // Alignment gap show and set join the same working-copy edits.
            // Versions and submissions add the head, revisions, exports, governance,
            // and pinned attachments to this same model workflow.
            // The integrated profile contains 55 lifecycle leaves and two
            // bootstrap tools; each leaf belongs to this model workflow.
            // Combined-model verbs remain in the exact lifecycle allowlist.
            // This profile loads their contracts lazily through two scoped
            // chapter routers instead of widening its 57-tool ceiling.
            Self::GridLocalModel => 57,
            // Seventeen geospatial leaves plus bootstrap: the same answer
            // can be kept as GeoJSON or converted to the analytical
            // GeoParquet format without switching MCP profiles.
            // Native terrain sampling (describe + sample) prepares model ground
            // from the same governed sources: two more leaves, same workflow.
            Self::Datasets => 24,
            // The file-in/file-out engine workflow: sixteen leaves plus both
            // bootstrap tools. `dsgrid replace-structure` belongs beside
            // `import-structure`; `dsgrid.backup.preview` inspects the native
            // package before the same file workflow imports or restores it.
            // Exact retained-source cable reconciliation adds one native package leaf.
            Self::GridNative => 19,
            // Eleven existing guided leaves, eleven primary/zoom/preset/JSON
            // leaves and nine resolver/catalogue leaves make the closed
            // authoring and governance workflow (31 leaves plus bootstrap).
            Self::Styles => STYLE_COMMANDS.len() + 2,
            _ => 16,
        }
    }

    /// Only this large lifecycle profile uses scoped chapter disclosure.
    /// The command allowlist below remains its complete authority boundary.
    fn router_chapters(self) -> &'static [Chapter] {
        match self {
            Self::GridLocalModel => &[Chapter::GridModel, Chapter::Design],
            _ => &[],
        }
    }

    pub fn includes(self, tool: &Tool) -> bool {
        match self {
            Self::AuthContext => AUTH_CONTEXT_COMMANDS.contains(&tool.id.as_str()),
            Self::AdminBounds => ADMIN_BOUNDS_COMMANDS.contains(&tool.id.as_str()),
            Self::Datasets => DATASET_COMMANDS.contains(&tool.id.as_str()),
            Self::Installations => INSTALLATION_COMMANDS.contains(&tool.id.as_str()),
            Self::GridNative => {
                tool.authority == ds_cli_contract::spec::Authority::None
                    && (tool.id.starts_with("dsgrid.") || tool.id.starts_with("dsgrid-exchange."))
                    // Reviewed corrections have their own mandatory-guard
                    // profile. Native sync is an exchange delivery act, not
                    // part of the bounded native-model reading/editing set.
                    && !matches!(tool.id.as_str(),
                        "dsgrid.apply-batch" | "dsgrid.apply-correction" | "dsgrid-exchange.sync" | "dsgrid.reconcile-cable-source")
                    // A visualization-only view of a truncated spotting
                    // receipt is the spotting workflow's, published by the
                    // `grid` profile `ds-grid-spotting` names; it is not a
                    // native-model read or edit.
                    && tool.id != "dsgrid.spotting.preview-receipt"
                    && tool.id != "dsgrid.spotting.apply-receipt"
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
            Self::GridCorrections => GRID_CORRECTION_COMMANDS.contains(&tool.id.as_str()),
            Self::GridProfile => GRID_PROFILE_COMMANDS.contains(&tool.id.as_str()),
            Self::Grid => {
                matches!(tool.chapter, Chapter::GridModel | Chapter::Reports)
                    // Model library adoption belongs to the scoped lifecycle
                    // router, never to the bounded spotting/report profile.
                    && !tool.id.starts_with("library.model.")
                    && !matches!(tool.id.as_str(),
                        "dsgrid.apply-batch" | "dsgrid.apply-correction" | "dsgrid-exchange.sync")
                    && tool.id != "dsgrid.reconcile-cable-source"
                    // File-template compilation and application belong to
                    // `grid-native`; keep this whole family out of the broad
                    // model/report workflow instead of expanding its budget.
                    && !tool.id.starts_with("dsgrid.template.")
                    && !PROJECT_OPERATIONS_COMMANDS.contains(&tool.id.as_str())
                    && tool.id != "report.spatial.workbook"
                    // Printing has its own workflow profile and Reports router;
                    // changing that profile must not expand the Grid surface.
                    && !PRINTING_COMMANDS.contains(&tool.id.as_str())
                    && !REPORT_CONFIGURATION_COMMANDS.contains(&tool.id.as_str())
                    && !tool.id.starts_with("desktop.printing.")
                    && !tool.id.starts_with("report.layout.")
                    // The governed standard and A4 print documents are
                    // printing reads, never Grid leaves.
                    && !tool.id.starts_with("report.standard.")
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
                    // The governed head lifecycle (versions, retire, restore,
                    // show, compare, exports) lives with publication in
                    // `grid-local-model`; list and download stay here.
                    && !(tool.id.starts_with("dsgrid.project.")
                        && GRID_LOCAL_MODEL_COMMANDS.contains(&tool.id.as_str()))
            }
            Self::Printing => PRINTING_COMMANDS.contains(&tool.id.as_str()),
            Self::PrintingMaps => PRINTING_MAP_COMMANDS.contains(&tool.id.as_str()),
            Self::GridLocalModel => GRID_LOCAL_MODEL_COMMANDS.contains(&tool.id.as_str()),
            Self::GridClearance => GRID_CLEARANCE_COMMANDS.contains(&tool.id.as_str()),
            Self::Survey => SURVEY_MAP_COMMANDS.contains(&tool.id.as_str()),
            Self::FormFactory => FORM_FACTORY_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyProjects => SURVEY_PROJECT_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyMedia => SURVEY_MEDIA_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyMigration => SURVEY_MIGRATION_COMMANDS.contains(&tool.id.as_str()),
            Self::Map => {
                tool.chapter == Chapter::MapPresentation
                    && tool.id != "map.profile.calculation"
                    && tool.id != "map.profile.edit"
                    && tool.id != "map.profile.save"
                    && tool.id != "map.profile.pick"
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
            Self::Project => {
                tool.chapter == Chapter::Project
                    && !tool.id.starts_with("auth.")
                    // The native runtime domain (renamed from `server`) is
                    // machine state, not project workflow.
                    && !tool.id.starts_with("core.")
                    && !NOTES_COMMANDS.contains(&tool.id.as_str())
                    && !tool.id.starts_with("account.")
                    && !tool.id.starts_with("messaging.")
                    && !PROJECT_CHECKLIST_COMMANDS.contains(&tool.id.as_str())
                    && (!PROJECT_TASK_OPERATIONS_COMMANDS.contains(&tool.id.as_str())
                        || tool.id == "pm.task.create")
                    && !(CORRESPONDENCE_COMMANDS.contains(&tool.id.as_str())
                        && tool.id != "pm.plan")
            }
            Self::ProjectTaskOperations => {
                PROJECT_TASK_OPERATIONS_COMMANDS.contains(&tool.id.as_str())
            }
            Self::ProjectChecklists => PROJECT_CHECKLIST_COMMANDS.contains(&tool.id.as_str()),
            Self::Notes => NOTES_COMMANDS.contains(&tool.id.as_str()),
            Self::Correspondence => CORRESPONDENCE_COMMANDS.contains(&tool.id.as_str()),
            // Push installations are this account's trusted receivers, listed
            // and revoked beside its trusted devices in `auth-context`.
            Self::Messaging => {
                tool.id.starts_with("messaging.")
                    && !AUTH_CONTEXT_COMMANDS.contains(&tool.id.as_str())
            }
            Self::Operations => {
                tool.chapter == Chapter::Operations
                    && !INSTALLATION_COMMANDS.contains(&tool.id.as_str())
                    // Native runtime administration (`core.*`, renamed from
                    // `server`) such as retiring a stopped Core's cache is a
                    // machine-owner step outside the running reliability
                    // workflow; it stays on the broad chapter surface.
                    && !tool.id.starts_with("core.")
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
            Self::Datasets => DATASET_COMMANDS,
            Self::Installations => INSTALLATION_COMMANDS,
            Self::Printing => PRINTING_COMMANDS,
            Self::PrintingMaps => PRINTING_MAP_COMMANDS,
            Self::GridLocalModel => GRID_LOCAL_MODEL_COMMANDS,
            Self::GridClearance => GRID_CLEARANCE_COMMANDS,
            Self::GridCorrections => GRID_CORRECTION_COMMANDS,
            Self::GridProfile => GRID_PROFILE_COMMANDS,
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
            Self::ProjectOperations => PROJECT_OPERATIONS_COMMANDS,
            Self::ProjectTaskOperations => PROJECT_TASK_OPERATIONS_COMMANDS,
            Self::ProjectChecklists => PROJECT_CHECKLIST_COMMANDS,
            Self::Notes => NOTES_COMMANDS,
            Self::Correspondence => CORRESPONDENCE_COMMANDS,
            Self::Grid
            | Self::GridNative
            | Self::Map
            | Self::Tiling
            | Self::Project
            | Self::Messaging
            | Self::Operations => &[],
        }
    }
}

// Principal handoff for an MCP host uses only the protected native session a
// person established in a trusted terminal. Password login is intentionally
// absent: an MCP child may inspect the non-secret AuthContext and refresh the
// visible project directory, but it may never receive password, approval
// authority, credential material, or a device-local project selector. Device
// begin/status/complete and inventory operate only through protected native
// state; `auth.link.approve` remains human-only and globally excluded.
const AUTH_CONTEXT_COMMANDS: &[&str] = &[
    "account.connect",
    "auth.status",
    "auth.link.begin",
    "auth.link.status",
    "auth.link.complete",
    "auth.device.list",
    "auth.device.read",
    "auth.device.revoke",
    "auth.project.list",
    // The account's push installations: which receivers it trusts and
    // revoking one, beside its trusted devices (moved from `messaging`).
    "messaging.installations",
    "messaging.installation.revoke",
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

// The retained Profile workflow includes its own explicit native job tickets.
// Keep the existing general map profile within its unchanged 16-tool bound.
const GRID_PROFILE_COMMANDS: &[&str] = &[
    "dsgrid.profile.open",
    "map.profile.view",
    "map.profile.set",
    "map.profile.select",
    "map.profile.pick",
    "map.profile.results",
    "map.profile.usage",
    "map.profile.issues",
    "map.profile.filter",
    "map.profile.retype",
    "map.profile.edit",
    "map.profile.save",
    "map.profile.calculation",
];

// One geospatial question can start in a held project layer or a governed
// BigQuery dataset and end as a GeoJSON layer or an aggregate workbook.
// These are typed commands, not an arbitrary SQL or file-system tool.
const DATASET_COMMANDS: &[&str] = &[
    "data.terrain.describe",
    "data.terrain.sample",
    "data.project-cache.status",
    "data.project-cache.query",
    "data.spatial.plan",
    "data.spatial.execute",
    "data.parcels.query",
    "data.customers.query",
    "data.upi.lookup",
    "data.admin-bounds.list",
    "data.admin-bounds.read",
    "data.inspect",
    "data.convert",
    "data.conversion-matrix",
    "dsgrid.project.geojson",
    "map.local.register",
    "map.local.list",
    "report.spatial.workbook",
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
    "style.categorical.plan",
    "style.categorical.set",
    "style.color-range.plan",
    "style.color-range.set",
    "style.zoom.plan",
    "style.zoom.set",
    "style.preset.plan",
    "style.preset.set",
    "style.instruction.schema",
    "style.resolve",
    "style.resolution.table",
    "style.purpose.index",
    "style.purpose.plan",
    "style.catalogue.manifest",
    "style.catalogue.seed.plan",
    "style.catalogue.seed.apply",
    "style.catalogue.a4.plan",
    "style.catalogue.a4.create",
    "style.catalogue.inventory",
    "style.catalogue.backup.create",
    "style.catalogue.backup.read",
    "style.catalogue.retirement.plan",
    "style.instruction.plan",
    "style.instruction.set",
];

/// Create-only source seeding and print cloning are one small publication
/// workflow, kept separate from ordinary Style Center edits.
const PRINT_STYLE_COMMANDS: &[&str] = &[
    "style.catalogue.a4.migration.plan",
    "style.catalogue.a4.migration.apply",
    "style.catalogue.binding.plan",
    "style.catalogue.binding.create",
    "style.seed.plan",
    "style.seed.create",
    "style.print.plan",
    "style.print.create",
    "style.print.versions.list",
    "style.print.versions.read",
    "style.print.versions.compare",
    "style.print.versions.restore",
];

// DS Grid working copies and governed project model lifecycle. Discover every
// project model by opaque ID before asking for that model's versions; display
// names are not identifiers and do not encode version numbers.
const GRID_LOCAL_MODEL_COMMANDS: &[&str] = &[
    "library.model.create",
    "library.model.attach",
    "library.model.detach",
    "library.model.clone",
    "library.model.show",
    "library.model.match",
    "library.model.impact-plan",
    "library.model.impact-apply",
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
    "dsgrid.project.list",
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
    // 2026-09-21 landings that widened the broad `grid` router past its
    // budget (31 tools against 27) without a profile decision: forgetting a
    // working copy, its village facts and its Profile labels are the same
    // working-copy workflow as the typed edits above and live here.
    "dsgrid.model.forget",
    "dsgrid.structure.admin-refresh",
    "dsgrid.profile.labels.set",
    // The governed head's own lifecycle beside its publication (2026-09-24):
    // `dsgrid project versions|retire|restore` (918a3b6) pushed the broad
    // `grid` router to 31 against 28; they live here with `publish-version`
    // and `prepare-project`, and the chapter router carries them regardless.
    "dsgrid.project.versions",
    "dsgrid.project.geojson",
    "dsgrid.project.retire",
    "dsgrid.project.restore",
    "dsgrid.profile.labels.show",
    // A version's package assets (2026-09-25): the original PLS-CADD upload
    // it carries, and the delivered backup attached to the snapshot it
    // describes, for a local package or an exact governed revision.
    "dsgrid.asset.list",
    "dsgrid.asset.extract",
    "dsgrid.asset.attach",
    "dsgrid.asset.detach",
    "dsgrid.project.asset.list",
    "dsgrid.project.asset.extract",
    // The multiple-alignment gap (2026-09-25) is a typed edit of the working
    // copy, read and set like its Profile labels.
    "dsgrid.alignment.gap.show",
    "dsgrid.alignment.gap.set",
    // Versions and submissions (2026-09-25): one workflow from saving a
    // revision to marking the submitted version and reading back what it
    // carries — the head and a revision without bytes, a revision diff, the
    // revision's exports, the working copy's PLS-CADD unlink, the MV
    // governance vN and the attachments pinned to a content revision.
    "dsgrid.project.show",
    "dsgrid.project.compare",
    "dsgrid.project.exports.list",
    "dsgrid.project.exports.publish",
    "dsgrid.project.exports.download",
    "dsgrid.project.bump-version",
    "dsgrid.project.update",
    "dsgrid.project.set-approval",
    "dsgrid.project.backup.download",
    "dsgrid.model.unlink",
    "design.version.import-prepare",
    "design.version.import",
    "design.version.catalog",
    "design.version.revision.show",
    "design.version.revision.publish",
    "design.version.renumber",
    "design.version.revision.bind",
    "design.version.revision.promote",
    "design.version.revision.compare",
    "design.version.list",
    "design.version.status",
    "design.version.show",
    "design.version.compare",
    "design.version.begin",
    "design.version.begin-batch",
    "design.version.summaries",
    "design.attachment.list",
    "design.attachment.publish",
    "design.attachment.download",
    "design.attachment.list-project",
    "design.attachment.show",
    "design.attachment.versions",
    "design.attachment.set-latest",
    "design.attachment.retire",
    // Deliberate editable snapshot composition belongs to model lifecycle.
    "dsgrid.model.combine",
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
    "dsgrid.model.forget",
    "dsgrid.structure.admin-refresh",
    "dsgrid.profile.labels.set",
    "dsgrid.profile.labels.show",
    "dsgrid.asset.list",
    "dsgrid.asset.extract",
    "dsgrid.asset.attach",
    "dsgrid.asset.detach",
    "dsgrid.project.asset.list",
    "dsgrid.project.asset.extract",
    "dsgrid.alignment.gap.show",
    "dsgrid.alignment.gap.set",
    "dsgrid.model.unlink",
    "dsgrid.model.combine",
];

// Program contract 03: feature codes and clearance across the PLS-CADD
// boundary — the table report, the standard import, the migration of survey
// tokens, the FEA export, the clearance criteria (set and show) and the
// clearance report. One operator workflow over one working copy, from the
// consultant's "clearances not configured" to the list of violations by
// feature code; the surrounding file workflow stays in `grid-native`.
const GRID_CLEARANCE_COMMANDS: &[&str] = &[
    "dsgrid.feature-codes.report",
    "dsgrid.feature-codes.cleared-forest-offset.set",
    "dsgrid.feature-codes.import",
    "dsgrid.feature-codes.migrate",
    "dsgrid.feature-codes.export",
    "dsgrid.criteria.show",
    "dsgrid.criteria.clearance.set",
    "dsgrid.analyse.clearance",
];

/// One spotted-model correction workflow, kept below the typed MCP tool
/// ceiling even when the broad grid profiles grow. Native sync and final PLS
/// acceptance remain separate operator steps.
const GRID_CORRECTION_COMMANDS: &[&str] = &[
    "dsgrid-exchange.inspect",
    "dsgrid-exchange.plan",
    "dsgrid-exchange.convert",
    "dsgrid.inspect",
    "dsgrid.validate",
    "dsgrid.describe",
    "dsgrid.run",
    "dsgrid.report.structures",
    "dsgrid.analyse.clearance",
    "dsgrid.apply-correction",
    // Source-attested cable mechanics are an exact package correction, not
    // part of the broad grid discovery router.
    "dsgrid.reconcile-cable-source",
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
    // The retained native Survey entry queue this machine captured offline.
    "core.survey.queue",
    // The survey dashboard numbers for one project.
    "survey.dashboard",
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
    // The retained overlay registry (6ccc4c9c): read it, edit it locally.
    "map.layer.registry",
    "map.layer.registry-edit",
];

const FORM_FACTORY_COMMANDS: &[&str] = &[
    "survey.form.validate",
    "survey.forms.list",
    "survey.form.read",
    "survey.form.types",
    "survey.form.create",
    "survey.form.update",
    "survey.form.lifecycle",
    "survey.form.purge-plan",
    "survey.forms.purge-candidates",
    "survey.form.purge",
];

// Survey photos as one operator workflow: what this machine holds, one
// photo's details, the one rotation (held locally first) and its
// publication, plus the offline file rotation. Split out of
// `survey-projects` on 2026-09-20 when the moments leaves would have taken
// that profile past its bound.
const SURVEY_MEDIA_COMMANDS: &[&str] = &[
    "survey.entries.read",
    "survey.local.status",
    "survey.moments.list",
    "survey.moments.read",
    "survey.photo.rotate",
    "survey.photo.fetch",
    "survey.photo.publish",
    "survey.photo.rotate-local",
];

const SURVEY_PROJECT_COMMANDS: &[&str] = &[
    "survey.member-grant.read",
    "survey.member-grant.plan",
    "survey.member-grant.apply",
    "survey.query",
    "survey.entries.select",
    "survey.entries.changes",
    "survey.entries.create",
    "survey.entries.delete-plan",
    "survey.entries.update-plan",
    "survey.entries.update",
    "survey.entries.delete",
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

// Getting survey data into a project: canonical NDJSON import, or a
// project-to-project copy planned then applied (both projects explicit).
const SURVEY_MIGRATION_COMMANDS: &[&str] = &[
    "survey.entries.import",
    "survey.migrate.plan",
    "survey.migrate.apply",
];

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
// reports name their project on each headless request. (`design.transformer.download`
// left on 2026-09-20: it only ever warmed a window's private room cache, and
// the native report path reads rooms from the service.) Kept out of
// `design-edit` (already at its bound) and out of the `grid` chapter router
// so neither grows; an agent doing background delivery work gets this narrow
// profile.
const CORRESPONDENCE_COMMANDS: &[&str] = &[
    "pm.plan",
    "pm.party.list",
    "pm.party.create",
    "pm.party.update",
    "pm.record.list",
    "pm.record.read",
    "pm.record.thread",
    "pm.record.create",
    "pm.record.reply",
    "pm.record.update",
    "pm.task.block",
    "pm.task.unblock",
];

// Nine selected item/copy/mail leaves share one closed, bounded profile.
// Keep them out of the broad Project profile without increasing its budget.
const PROJECT_CHECKLIST_COMMANDS: &[&str] = &[
    "pm.task.checklist.read",
    "pm.task.checklist.edit",
    "pm.task.checklist.promote",
    "pm.note.checklist.read",
    "pm.note.checklist.edit",
    "notes.checklist.read",
    "notes.checklist.edit",
    "pm.document.copy",
    "pm.mail-thread.ingest",
];

/// Private notes and shared project notes as one note-keeping workflow: write,
/// read, query, version, back up, retire, restore and consolidate. Their
/// checklist leaves stay in `project-checklists`. Split out of `project` on
/// 2026-10-19 when the two note families took it past its bound.
const NOTES_COMMANDS: &[&str] = &[
    "notes.create",
    "notes.update",
    "notes.list",
    "notes.read",
    "notes.versions",
    "notes.version.read",
    "notes.version.restore",
    "notes.retire",
    "notes.restore",
    "notes.backup",
    "notes.backup.read",
    "notes.backup.restore",
    "pm.note.create",
    "pm.note.read",
    "pm.note.query",
    "pm.note.context",
    "pm.note.versions",
    "pm.note.update",
    "pm.note.backup",
    "pm.note.retire",
    "pm.note.restore",
    "pm.note.consolidate",
];

/// Task creation with typed geometry, plus task geometry and execution
/// decisions. Kept apart from correspondence and broad project planning.
const PROJECT_TASK_OPERATIONS_COMMANDS: &[&str] = &[
    "pm.task.detach",
    "pm.task.create",
    "pm.task.propose",
    "pm.task.request-admission",
    "pm.task.admit",
    "pm.task.decline",
    "pm.task.log-hours",
    "pm.task.geometry.read",
    "pm.task.geometry.clear",
    "pm.model-link.add",
    "pm.model-link.remove",
    "pm.model-link.list",
];

const PROJECT_OPERATIONS_COMMANDS: &[&str] = &[
    "report.artifact.remove",
    "design.status",
    "design.transformer.inventory",
    "design.transformer.retire",
    "design.transformer.restore",
    "report.project.scope",
    "report.project.compounded",
    // The Combined Report computed on this machine and sealed into Sync.
    "report.project.combined",
    "report.project.archives",
    "report.publication.list",
    "report.publication.show",
    "report.project.export",
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

/// Project printing configuration belongs to `printing`, but stays outside
/// the broad Grid router, whose chapter selection also includes Reports.
const REPORT_CONFIGURATION_COMMANDS: &[&str] = &[
    "report.project.settings",
    "report.project.outputs.set",
    "report.project.mv-setup.set",
    "report.project.mv-setup.resolve",
];

const DESIGN_RUN_COMMANDS: &[&str] = &[
    "design.intake.upload",
    "design.lv.project-export",
    "design.lv.process",
    "design.lv.voltage-drop",
    "map.design.process",
    "map.design.batch.process",
    "map.design.batch.save",
    "map.design.save",
    "map.design.list",
    "map.design.report",
    "map.design.attach-print",
];

const SOLAR_APPLICATION_COMMANDS: &[&str] = &["solar.application", "solar.application.schema"];
const SOLAR_RUN_COMMANDS: &[&str] = &[
    "solar.engine",
    "solar.project.clean",
    "solar.run",
    // Offline calculation of one prepared city: the same run, one file task.
    "solar.calculate-prepared",
    "solar.result.compare",
    "solar.results.read",
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
    "solar.reference.plan",
    "solar.reference.import",
    "solar.project.run",
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
    "solar.portfolio.published.read",
];

const SOLAR_DELIVERY_COMMANDS: &[&str] = &[
    "solar.project.outbox",
    "solar.project.sync",
    "solar.project.sync.rebase",
    "solar.portfolio.list",
    "solar.portfolio.create",
    "solar.portfolio.update",
    "solar.portfolio.delete",
    "solar.portfolio.analysis",
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
    call_timeout: Duration,
}

/// The command a `ds` child answers for, as its envelope names it, and the
/// effect class that decides what a stopped or oversized answer means.
struct Answering<'a> {
    id: &'a str,
    contract: u32,
    effect: Effect,
}

impl<'a> Answering<'a> {
    fn tool(tool: &'a Tool) -> Self {
        Self {
            id: &tool.id,
            contract: tool.descriptor["contract"].as_u64().unwrap_or(1) as u32,
            effect: tool.effect,
        }
    }

    /// A describe or diagnostic: the CLI's own read-only meta commands.
    fn meta(id: &'a str) -> Self {
        Self {
            id,
            contract: 1,
            effect: Effect::ReadOnly,
        }
    }
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
            let published = if profile.router_chapters().is_empty() {
                commands.len() + 2
            } else {
                profile.router_chapters().len() + 2
            };
            if published > profile.tool_limit() {
                return Err(Failure::failed(
                    "mcp_profile_too_broad",
                    format!(
                        "profile `{}` would publish {} tools including `ds_catalog` and `ds_diagnostics`",
                        profile.token(),
                        published
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
            call_timeout: DEFAULT_CALL_TIMEOUT,
        })
    }

    pub fn with_identity(mut self, identity: Value) -> Self {
        self.identity = identity;
        self
    }

    pub fn with_call_timeout(mut self, timeout: Duration) -> Self {
        self.call_timeout = timeout;
        self
    }

    pub fn call_timeout(&self) -> Duration {
        self.call_timeout
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
            (Exposure::Commands, Some(profile)) => {
                if profile.router_chapters().is_empty() {
                    self.commands.len() + 2
                } else {
                    profile.router_chapters().len() + 2
                }
            }
            (Exposure::Commands, None) => self.commands.len() + 1,
            (Exposure::Chapters, Some(_)) => 0,
        }
    }

    pub fn instructions(&self) -> String {
        let instructions = match (self.exposure, self.profile) {
            (Exposure::Chapters, None) => "Use ds_catalog for bounded discovery, call the selected chapter with operation=describe, then operation=invoke. The canonical command descriptor governs arguments, authority, effect, confirmation and refusals; branch on the returned DS envelope.".to_string(),
            (Exposure::Commands, Some(profile)) if !profile.router_chapters().is_empty() => format!(
                "This is the scoped `{}` lifecycle profile. Use ds_catalog for bounded discovery, then the returned chapter router with operation=describe or operation=invoke and the exact command id. Only this profile's allowlisted commands are reachable. Pass confirm=true only when declared and authorized for that effect and scope; branch on the DS envelope.",
                profile.token()
            ),
            (Exposure::Commands, Some(profile)) => format!(
                "This is the typed `{}` profile. Use ds_catalog for bounded discovery, then call the advertised command tool directly. Pass confirm=true only when the command declares it and the user's intent authorizes that exact effect and scope. Branch on the returned DS envelope.",
                profile.token()
            ),
            (Exposure::Commands, None) => "Compatibility command exposure: every advertised tool is one canonical ds command generated from its live descriptor. Pass confirm=true only when declared, branch on the returned DS envelope, and follow typed remedies.".to_string(),
            (Exposure::Chapters, Some(_)) => unreachable!("invalid surface is refused"),
        };
        format!(
            "{instructions} {DEVICE_LINK_GUIDANCE} {PROJECT_WORK_GUIDANCE} {FEEDBACK_GUIDANCE} {REGRESSION_GUIDANCE} {BOOKLET_GUIDANCE}"
        )
    }

    pub fn tool_list(&self) -> Vec<Value> {
        match (self.exposure, self.profile) {
            (Exposure::Chapters, None) => std::iter::once(catalog_tool_json())
                .chain(std::iter::once(diagnostics_tool_json()))
                .chain(
                    ROUTED_CHAPTERS
                        .iter()
                        .map(|chapter| self.chapter_tool_json(*chapter)),
                )
                .collect(),
            (Exposure::Commands, Some(profile)) if !profile.router_chapters().is_empty() => {
                std::iter::once(catalog_tool_json())
                    .chain(std::iter::once(diagnostics_tool_json()))
                    .chain(
                        profile
                            .router_chapters()
                            .iter()
                            .map(|chapter| self.chapter_tool_json(*chapter)),
                    )
                    .collect()
            }
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

    /// The annotations of one chapter router, derived from the commands it
    /// routes: read-only and idempotent only if every one of them is,
    /// destructive if any one of them is.
    fn chapter_tool_json(&self, chapter: Chapter) -> Value {
        let hints = self
            .commands
            .iter()
            .filter(|tool| tool.chapter == chapter)
            .map(|tool| tools::hints(tool.effect))
            .fold(
                tools::Hints {
                    read_only: true,
                    destructive: false,
                    idempotent: true,
                },
                |all, one| tools::Hints {
                    read_only: all.read_only && one.read_only,
                    destructive: all.destructive || one.destructive,
                    idempotent: all.idempotent && one.idempotent,
                },
            );
        chapter_tool_json(chapter, hints)
    }

    pub fn call(
        &self,
        name: &str,
        arguments: &Value,
        executable: &PathBuf,
    ) -> Result<Value, (i64, String)> {
        self.call_observed(name, arguments, executable, &mut |_| {})
    }

    /// [`Surface::call`], reporting the elapsed time of a long-running call
    /// to `tick` every [`tools::PROGRESS_INTERVAL`].
    ///
    /// `Err` is a protocol error and is reserved for routing: a tool this
    /// server does not publish, or a router envelope that names no command.
    /// Once a command is resolved, everything wrong with how it was called is
    /// that command's refusal — an `isError` result carrying a DS envelope
    /// with a code and a remedy, exactly as the CLI answers a bad flag.
    pub fn call_observed(
        &self,
        name: &str,
        arguments: &Value,
        executable: &PathBuf,
        tick: &mut dyn FnMut(Duration),
    ) -> Result<Value, (i64, String)> {
        if name == "ds_diagnostics" {
            return self.call_diagnostics(arguments, executable, tick);
        }
        if name == "ds_catalog" && (self.exposure == Exposure::Chapters || self.profile.is_some()) {
            return self.call_catalog(arguments, executable);
        }
        match self.exposure {
            Exposure::Commands => {
                if let Some(profile) = self
                    .profile
                    .filter(|profile| !profile.router_chapters().is_empty())
                {
                    let Some(chapter) = profile
                        .router_chapters()
                        .iter()
                        .copied()
                        .find(|chapter| chapter_tool_name(*chapter) == name)
                    else {
                        return Err((-32602, format!("unknown tool: {name}")));
                    };
                    // call_chapter resolves only self.commands: same-chapter
                    // commands outside the profile never inherit access.
                    return self.call_chapter(chapter, arguments, executable, tick);
                }
                let Some(tool) = self.commands.iter().find(|tool| tool.name == name) else {
                    return Err((-32602, format!("unknown tool: {name}")));
                };
                invoke_leaf(tool, arguments, executable, self.call_timeout, tick)
            }
            Exposure::Chapters => {
                let Some(chapter) = ROUTED_CHAPTERS
                    .iter()
                    .copied()
                    .find(|chapter| chapter_tool_name(*chapter) == name)
                else {
                    return Err((-32602, format!("unknown tool: {name}")));
                };
                self.call_chapter(chapter, arguments, executable, tick)
            }
        }
    }

    fn call_chapter(
        &self,
        chapter: Chapter,
        arguments: &Value,
        executable: &PathBuf,
        tick: &mut dyn FnMut(Duration),
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
                invoke_argv(
                    &argv,
                    executable,
                    &Answering::meta("capabilities"),
                    self.call_timeout,
                    tick,
                )
            }
            "invoke" => {
                let Some(mut nested) = nested.as_object().cloned() else {
                    return Ok(arguments_refusal(
                        tool,
                        "`arguments` must be an object".to_string(),
                    ));
                };
                // A command's own `confirm` input travels with its arguments;
                // the MCP confirmation belongs to the envelope.
                if nested.contains_key(CONFIRM_PROPERTY) && !tool.owns_confirm_input() {
                    return Ok(arguments_refusal(
                        tool,
                        format!(
                            "put `{CONFIRM_PROPERTY}` in the chapter envelope, not inside `arguments`"
                        ),
                    ));
                }
                let nested_arguments = Value::Object(nested.clone());
                let confirmation_required = match tool.confirmation_required_for(&nested_arguments)
                {
                    Ok(required) => required,
                    Err(message) => return Ok(arguments_refusal(tool, message)),
                };
                if confirm {
                    if !confirmation_required {
                        return Ok(arguments_refusal(
                            tool,
                            format!("`{command}` does not accept confirmation for this invocation"),
                        ));
                    }
                    nested.insert(CONFIRM_PROPERTY.to_string(), Value::Bool(true));
                }
                invoke_leaf(
                    tool,
                    &Value::Object(nested),
                    executable,
                    self.call_timeout,
                    tick,
                )
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
            let mut matches: Vec<([usize; 4], &Tool)> = visible
                .filter_map(|tool| catalog_search_score(tool, &terms).map(|score| (score, tool)))
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
        tick: &mut dyn FnMut(Duration),
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
        let (id, argv) = match operation.as_str() {
            "doctor" => ("doctor", vec!["doctor", "--output", "json"]),
            "shell.status" => ("shell.status", vec!["shell", "status", "--output", "json"]),
            "capabilities" => ("capabilities", vec!["capabilities", "--output", "json"]),
            _ => {
                return Err((
                    -32602,
                    "`operation` must be `identity`, `doctor`, `shell.status`, or `capabilities`"
                        .to_string(),
                ));
            }
        };
        let argv = argv.into_iter().map(str::to_string).collect::<Vec<_>>();
        invoke_argv(
            &argv,
            executable,
            &Answering::meta(id),
            self.call_timeout,
            tick,
        )
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

/// Search authored command intent, not the generated execution/refusal guidance
/// shared by unrelated tools. Keep OR matching; prefer ID/summary coverage,
/// then ID matches, summary matches and purpose-only relevance.
fn catalog_search_score(tool: &Tool, terms: &[String]) -> Option<[usize; 4]> {
    let id = tool.id.to_lowercase();
    let summary = tool
        .descriptor
        .get("summary")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    let purpose = tool
        .descriptor
        .get("purpose")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_lowercase();
    // Whole words, so `in` does not match `machine` and `sign` does not match
    // `design`; a term that itself carries punctuation (an id fragment such as
    // `solar.run`) still matches as written.
    let matches = |text: &str, term: &str| {
        if term.chars().all(char::is_alphanumeric) {
            text.split(|c: char| !c.is_alphanumeric()).any(|word| word == term)
        } else {
            text.contains(term)
        }
    };
    let mut score = [0; 4];
    for term in terms {
        let in_id = matches(&id, term);
        let in_summary = matches(&summary, term);
        score[0] += usize::from(in_id || in_summary);
        score[1] += usize::from(in_id);
        score[2] += usize::from(in_summary);
        score[3] += usize::from(matches(&purpose, term));
    }
    score.iter().any(|count| *count > 0).then_some(score)
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
    timeout: Duration,
    tick: &mut dyn FnMut(Duration),
) -> Result<Value, (i64, String)> {
    let confirmation_required = match tool.confirmation_required_for(arguments) {
        Ok(required) => required,
        Err(message) => return Ok(arguments_refusal(tool, message)),
    };
    let argv = match tools::argv_for_call(tool, arguments) {
        Ok(argv) => argv,
        Err(message) => return Ok(arguments_refusal(tool, message)),
    };
    let answering = Answering::tool(tool);
    // The registry owns confirmation and refuses before any handler opens a
    // bridge. Preserve that ordering here too: an unconfirmed paired write is
    // an input refusal, never a reason to start a desktop.
    if confirmation_required
        && !arguments
            .get(tools::CONFIRM_PROPERTY)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    {
        return invoke_argv(&argv, executable, &answering, timeout, tick);
    }
    if let Err(failure) = tools::ensure_desktop(tool, arguments, executable) {
        return Ok(failure_result(tool, &failure));
    }
    invoke_argv(&argv, executable, &answering, timeout, tick)
}

/// A resolved command's refusal of the arguments it was sent. It is a tool
/// result rather than a protocol error: the tool exists and was called, and
/// what the caller must change is an input — which is what a DS envelope's
/// code and remedy are for.
fn arguments_refusal(tool: &Tool, message: String) -> Value {
    let failure = Failure::invalid(crate::ARGUMENTS_INVALID.code, message)
        .remedy(crate::ARGUMENTS_INVALID.remedy)
        .next(format!("ds capabilities {}", tool.id));
    failure_result(tool, &failure)
}

/// Whether one sentence would send a person to a terminal sign-in.
pub fn names_terminal_sign_in(text: &str) -> bool {
    let lower = text.to_lowercase();
    TERMINAL_SIGN_IN_WORDS
        .iter()
        .any(|banned| lower.contains(banned))
}

// Belt and braces. Every remedy `ds` emits already names the device link,
// and `crates/ds/tests/mcp.rs` proves the published surface carries no
// terminal sign-in advice — so in production this rewrites nothing. It stays
// because the cost of one sentence slipping through is a person told to open
// a terminal, and that is the failure this whole surface exists to prevent.
//
// Only advisory fields are touched: `remedy`, `next`, `message` and `when`
// inside an envelope's `error` or a descriptor's refusals, plus the prose of
// a descriptor and the `next` of a result. Data a command returns — a
// feedback report's text, a survey answer — is never rewritten: those are
// facts about the world, not advice to the caller.
fn mcp_device_link_guidance(value: &mut Value) {
    let Some(fields) = value.as_object_mut() else {
        return;
    };
    if let Some(error) = fields.get_mut("error") {
        scrub_advice(error);
    }
    if let Some(data) = fields.get_mut("data") {
        if let Some(command) = data.get_mut("command") {
            scrub_descriptor(command);
        }
        if let Some(commands) = data.get_mut("commands").and_then(Value::as_array_mut) {
            for command in commands {
                scrub_descriptor(command);
            }
        }
        if let Some(object) = data.as_object_mut() {
            for key in ["next", "remedy", "approve"] {
                if let Some(Value::String(text)) = object.get_mut(key)
                    && names_terminal_sign_in(text)
                {
                    *text = match key {
                        "next" => DEVICE_LINK_NEXT.to_string(),
                        _ => DEVICE_LINK_REMEDY.to_string(),
                    };
                }
            }
        }
    }
}

/// The advisory fields of one refusal or error body.
fn scrub_advice(value: &mut Value) {
    let Some(fields) = value.as_object_mut() else {
        return;
    };
    for (name, child) in fields.iter_mut() {
        match (name.as_str(), child) {
            ("next", Value::String(text)) if names_terminal_sign_in(text) => {
                *text = DEVICE_LINK_NEXT.to_string();
            }
            ("next", Value::Array(items)) => {
                for item in items {
                    if let Value::String(text) = item
                        && names_terminal_sign_in(text)
                    {
                        *text = DEVICE_LINK_NEXT.to_string();
                    }
                }
            }
            ("remedy" | "when", Value::String(text)) if names_terminal_sign_in(text) => {
                *text = DEVICE_LINK_REMEDY.to_string();
            }
            ("message", Value::String(text)) if names_terminal_sign_in(text) => {
                *text = format!("sign-in is needed: {DEVICE_LINK_REMEDY}");
            }
            _ => {}
        }
    }
}

/// The prose of one command descriptor: its own sentences, its inputs'
/// summaries, its examples and its refusals.
pub(crate) fn scrub_descriptor(command: &mut Value) {
    let Some(fields) = command.as_object_mut() else {
        return;
    };
    for key in ["summary", "purpose", "output", "description"] {
        if let Some(Value::String(text)) = fields.get_mut(key)
            && names_terminal_sign_in(text)
        {
            *text = DEVICE_LINK_REMEDY.to_string();
        }
    }
    for key in ["inputs", "args", "examples"] {
        for item in fields
            .get_mut(key)
            .and_then(Value::as_array_mut)
            .into_iter()
            .flatten()
        {
            if let Some(object) = item.as_object_mut() {
                for field in ["summary", "note", "command"] {
                    if let Some(Value::String(text)) = object.get_mut(field)
                        && names_terminal_sign_in(text)
                    {
                        *text = DEVICE_LINK_REMEDY.to_string();
                    }
                }
            }
        }
    }
    for refusal in fields
        .get_mut("refusals")
        .and_then(Value::as_array_mut)
        .into_iter()
        .flatten()
    {
        scrub_advice(refusal);
    }
}

fn failure_result(tool: &Tool, failure: &Failure) -> Value {
    let answering = Answering::tool(tool);
    failure_value(&answering, failure)
}

fn failure_value(answering: &Answering<'_>, failure: &Failure) -> Value {
    let mut envelope =
        serde_json::to_value(error_envelope(answering.id, answering.contract, failure))
            .unwrap_or_else(|_| json!({ "status": "error" }));
    mcp_device_link_guidance(&mut envelope);
    let text = serde_json::to_string(&envelope).unwrap_or_else(|_| "{}".to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": envelope,
        "isError": true,
    })
}

fn invoke_argv(
    argv: &[String],
    executable: &PathBuf,
    answering: &Answering<'_>,
    timeout: Duration,
    tick: &mut dyn FnMut(Duration),
) -> Result<Value, (i64, String)> {
    let ran = tools::run_cli_bounded(executable, argv, timeout, tick)
        .map_err(|message| (-32000, message))?;
    if ran.timed_out {
        return Ok(failure_value(answering, &timed_out(answering, timeout)));
    }
    if ran.overflowed {
        return Ok(failure_value(answering, &too_large(answering)));
    }
    let mut envelope: Option<Value> = serde_json::from_str(ran.stdout.trim()).ok();
    let is_error = ran.code != 0
        || envelope
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            != Some("ok");
    if let Some(envelope) = &mut envelope {
        mcp_device_link_guidance(envelope);
        redact_credentials(envelope);
        bound_envelope(envelope, MAX_RESULT_BYTES);
    }
    let text = envelope.as_ref().map_or_else(
        || {
            let raw = if ran.stdout.trim().is_empty() {
                ran.stderr.trim()
            } else {
                ran.stdout.trim()
            };
            let mut text = bounded_text(raw, MAX_FALLBACK_TEXT_BYTES);
            redact_bearer(&mut text);
            text
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

/// The refusal for a child stopped at the call bound. What it means depends
/// on the effect: a read can simply be retried narrower, while a writing
/// command may have taken part of its effect, so its caller re-reads first.
fn timed_out(answering: &Answering<'_>, timeout: Duration) -> Failure {
    let message = format!(
        "`{}` ran past the {} s call bound and was stopped; an owner process it started may still be finishing",
        answering.id,
        timeout.as_secs()
    );
    let detail = json!({
        "timeout_seconds": timeout.as_secs(),
        "effect": answering.effect.token(),
    });
    if tools::hints(answering.effect).read_only {
        Failure::unavailable(crate::CALL_TIMED_OUT.code, message)
            .remedy("narrow the request, or raise `ds mcp serve --call-timeout` for long work")
            .detail(detail)
    } else {
        Failure::conflict(crate::CALL_TIMED_OUT.code, message)
            .remedy(crate::CALL_TIMED_OUT.remedy)
            .detail(detail)
    }
}

fn too_large(answering: &Answering<'_>) -> Failure {
    let effect = if tools::hints(answering.effect).read_only {
        "it changed nothing"
    } else {
        "its effect, if any, has happened"
    };
    Failure::unavailable(
        crate::RESULT_TOO_LARGE.code,
        format!(
            "`{}` wrote more than {} bytes, so its answer is not one whole envelope; {effect}",
            answering.id,
            tools::STDOUT_CAPTURE_LIMIT
        ),
    )
    .remedy(crate::RESULT_TOO_LARGE.remedy)
    .detail(json!({
        "limit_bytes": tools::STDOUT_CAPTURE_LIMIT,
        "effect": answering.effect.token(),
    }))
}

/// At most `limit` bytes of `text`, cut on a character boundary, with the
/// omission said rather than hidden.
fn bounded_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!(
        "{}… [{} bytes omitted by ds mcp]",
        &text[..end],
        text.len() - end
    )
}

/// Replace every credential in one answer, wherever it sits: a string under
/// a [`CREDENTIAL_KEYS`] key, and the token after `Bearer ` in any string.
/// Returns how many values were replaced.
pub fn redact_credentials(value: &mut Value) -> usize {
    match value {
        Value::Object(fields) => fields
            .iter_mut()
            .map(|(key, child)| {
                let credential = CREDENTIAL_KEYS
                    .iter()
                    .any(|name| key.eq_ignore_ascii_case(name));
                match child {
                    Value::String(text) if credential && !text.is_empty() => {
                        *text = REDACTED.to_string();
                        1
                    }
                    _ => redact_credentials(child),
                }
            })
            .sum(),
        Value::Array(items) => items.iter_mut().map(redact_credentials).sum(),
        Value::String(text) => redact_bearer(text),
        _ => 0,
    }
}

/// Replace the token after each `Bearer ` in `text`: an authorization header
/// quoted into an error message is still the credential.
fn redact_bearer(text: &mut String) -> usize {
    const SCHEME: &str = "bearer ";
    let token_char = |c: char| c.is_ascii_alphanumeric() || "-._~+/=".contains(c);
    let mut replaced = 0;
    let mut searched = 0;
    while let Some(found) = text[searched..].to_ascii_lowercase().find(SCHEME) {
        let start = searched + found + SCHEME.len();
        let length: usize = text[start..]
            .chars()
            .take_while(|c| token_char(*c))
            .map(char::len_utf8)
            .sum();
        // A word after "bearer" in prose is not a token; a credential is long.
        if length >= 16 {
            text.replace_range(start..start + length, REDACTED);
            replaced += 1;
            searched = start + REDACTED.len();
        } else {
            searched = start;
        }
    }
    replaced
}

/// Trim an envelope above `limit` serialized bytes until it fits, keeping it
/// one well-formed envelope: the largest array under `data` or
/// `error.detail` is shortened first, then the next, and only if no array is
/// left is the payload itself withheld. `status`, the error's code, message
/// and remedy are never touched. What was trimmed is reported in
/// `more.mcp_truncation` — `more` is where every DS answer reports that it is
/// not all of it — so nothing is silently lost.
pub fn bound_envelope(envelope: &mut Value, limit: usize) {
    let original = serialized_len(envelope);
    if original <= limit {
        return;
    }
    let mut arrays: Vec<(String, usize, usize)> = Vec::new();
    for _ in 0..64 {
        let total = serialized_len(envelope);
        if total <= limit {
            break;
        }
        let mut largest: Option<(usize, String)> = None;
        for root in ["/data", "/error/detail"] {
            if let Some(value) = envelope.pointer(root) {
                largest_array(value, &mut root.to_string(), &mut largest);
            }
        }
        let Some((size, pointer)) = largest else {
            break;
        };
        let Some(array) = envelope.pointer_mut(&pointer).and_then(Value::as_array_mut) else {
            break;
        };
        let length = array.len();
        // Keep the share of the array that fits, and always drop at least
        // one element so the loop ends.
        let excess = total - limit;
        let keep_bytes = size.saturating_sub(excess);
        let keep = (length * keep_bytes / size.max(1)).min(length.saturating_sub(1));
        array.truncate(keep);
        match arrays.iter_mut().find(|(seen, _, _)| *seen == pointer) {
            Some(entry) => entry.1 = keep,
            None => arrays.push((pointer, keep, length)),
        }
    }
    let mut withheld = Vec::new();
    if serialized_len(envelope) > limit {
        for pointer in ["/data", "/error/detail"] {
            if let Some(value) = envelope.pointer_mut(pointer)
                && !value.is_null()
            {
                *value = Value::Null;
                withheld.push(pointer);
            }
        }
    }
    let note = json!({
        "by": "ds mcp",
        "limit_bytes": limit,
        "original_bytes": original,
        "arrays": arrays
            .iter()
            .map(|(pointer, kept, total)| json!({ "pointer": pointer, "kept": kept, "total": total }))
            .collect::<Vec<_>>(),
        "withheld": withheld,
        "remedy": crate::RESULT_TOO_LARGE.remedy,
    });
    let Some(fields) = envelope.as_object_mut() else {
        return;
    };
    match fields.remove("more") {
        Some(Value::Object(mut more)) => {
            more.insert("mcp_truncation".to_string(), note);
            fields.insert("more".to_string(), Value::Object(more));
        }
        Some(other) => {
            fields.insert(
                "more".to_string(),
                json!({ "cli": other, "mcp_truncation": note }),
            );
        }
        None => {
            fields.insert("more".to_string(), json!({ "mcp_truncation": note }));
        }
    }
}

fn serialized_len(value: &Value) -> usize {
    serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len())
}

/// Find the non-empty array with the largest serialized size at or below
/// `value`, as a JSON pointer, returning `value`'s own serialized size.
fn largest_array(
    value: &Value,
    pointer: &mut String,
    largest: &mut Option<(usize, String)>,
) -> usize {
    match value {
        Value::Array(items) => {
            let mut size = 2 + items.len().saturating_sub(1);
            for (index, item) in items.iter().enumerate() {
                let before = pointer.len();
                pointer.push_str(&format!("/{index}"));
                size += largest_array(item, pointer, largest);
                pointer.truncate(before);
            }
            if !items.is_empty() && largest.as_ref().is_none_or(|(best, _)| size > *best) {
                *largest = Some((size, pointer.clone()));
            }
            size
        }
        Value::Object(fields) => {
            let mut size = 2 + fields.len().saturating_sub(1);
            for (key, child) in fields {
                let before = pointer.len();
                pointer.push('/');
                pointer.push_str(&key.replace('~', "~0").replace('/', "~1"));
                size += serialized_len(&Value::String(key.clone())) + 1;
                size += largest_array(child, pointer, largest);
                pointer.truncate(before);
            }
            size
        }
        scalar => serialized_len(scalar),
    }
}

fn value_result(value: Value, is_error: bool) -> Value {
    let text = serde_json::to_string(&value).unwrap_or_else(|_| "{}".to_string());
    json!({
        "content": [{ "type": "text", "text": text }],
        "structuredContent": value,
        "isError": is_error,
    })
}

/// One typed tool. Its annotations are [`tools::hints`] of its effect, so a
/// host deciding what it may run without asking reads the same class the
/// CLI's confirmation gate reads. `openWorldHint` is false for every tool:
/// a call reaches this executable, its owner engines, the paired
/// application or the DS service under the caller's own identity, and
/// nothing else.
pub fn leaf_tool_json(tool: &Tool) -> Value {
    let hints = tools::hints(tool.effect);
    json!({
        "name": tool.name,
        "title": tool.id,
        "description": tool.description,
        "inputSchema": tool.input_schema,
        "annotations": {
            "title": tool.id,
            "readOnlyHint": hints.read_only,
            "destructiveHint": hints.destructive,
            "idempotentHint": hints.idempotent,
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
        "annotations": { "title": "DS catalogue", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false }
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

fn chapter_tool_json(chapter: Chapter, hints: tools::Hints) -> Value {
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
        "annotations": {
            "title": chapter.token(),
            "readOnlyHint": hints.read_only,
            "destructiveHint": hints.destructive,
            "idempotentHint": hints.idempotent,
            "openWorldHint": false
        }
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
            "Process local GeoJSON with native vector measurement, buffering, line sampling and crossing tools; inspect or convert local files. Discover project-visible datasets, plan costed BigQuery reads and query held layers as GeoJSON. Local tools need no project; name the authorized project on project requests. Describe a command before invoking it."
        }
        Chapter::Project => {
            "Discover authorized projects and manage plans, tasks, assignments, and records for the project named in each request. Describe a command before invoking it."
        }
        Chapter::Assets => {
            "Browse the timeline, folders and version history of the documents a project holds; preview, classify, promote, link and ingest them. Describe a command before invoking it."
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
            "Read, stage, process, report, save, or discard transformer and LV design work; version, attach, tag and discuss transformers and MV DS Grid models (a submitted MV version's .bak binds to its content revision). Describe a command before invoking it."
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
            "Discover report tasks, export or bundle verified report artifacts, and publish the explicitly named project's Combined Report in the background. Describe a command before invoking it."
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
// Public city-data acquisition stays in the Data chapter and CLI; it is not
// a print consumer. Keep the printing profile within its existing tool bound.
const PRINTING_COMMANDS: &[&str] = &[
    "report.layout.render",
    "report.layout.context",
    "report.layout.list",
    "report.layout.get",
    "report.layout.edit",
    "report.layout.save",
    "report.transformers",
    "report.plan",
    "report.outbox.inventory",
    "data.project-cache.status",
    "data.project-cache.seed",
    "report.project.settings",
    "report.project.outputs.set",
    "report.project.mv-setup.set",
    "report.project.mv-setup.resolve",
    "report.project.export",
    "report.project.map-inputs",
    "report.project.crs",
    "report.voltage-drop-combined",
    "report.mv-frontmatter",
    "desktop.printing.artifact.read",
];

// Map sheet composition and publication are a small bounded companion to
// report production, so neither workflow loses its map delivery steps.
const PRINTING_MAP_COMMANDS: &[&str] = &[
    "assets.map.publish",
    "assets.maps",
    "map.print.schema",
    "map.design.attach-print",
];

#[cfg(test)]
#[path = "surface_tests.rs"]
mod tests;
