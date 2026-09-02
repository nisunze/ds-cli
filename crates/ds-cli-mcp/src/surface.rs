//! MCP publication shapes generated from the live command descriptors.

use std::path::PathBuf;

use ds_cli_contract::outcome::{Failure, error_envelope};
use ds_cli_contract::spec::Chapter;
use serde_json::{Map, Value, json};

use crate::tools::{self, CONFIRM_PROPERTY, Tool};

pub const EXPOSURES: &[&str] = &["chapters", "commands"];
pub const PROFILE_IDS: &[&str] = &[
    "auth-context",
    "grid",
    "grid-local-model",
    "pls",
    "pls-library",
    "library-governance",
    "survey",
    "form-factory",
    "survey-projects",
    "survey-migration",
    "design-edit",
    "design-run",
    "map",
    "layers",
    "tiling",
    "project",
    "solar-input",
    "solar-run",
    "solar-delivery",
    "operations",
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
    Grid,
    GridLocalModel,
    Pls,
    PlsLibrary,
    LibraryGovernance,
    Survey,
    FormFactory,
    SurveyProjects,
    SurveyMigration,
    DesignEdit,
    DesignRun,
    Map,
    Layers,
    Tiling,
    Project,
    SolarInput,
    SolarRun,
    SolarDelivery,
    Operations,
}

impl Profile {
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "auth-context" => Some(Self::AuthContext),
            "grid" => Some(Self::Grid),
            "grid-local-model" => Some(Self::GridLocalModel),
            "pls" => Some(Self::Pls),
            "pls-library" => Some(Self::PlsLibrary),
            "library-governance" => Some(Self::LibraryGovernance),
            "survey" => Some(Self::Survey),
            "form-factory" => Some(Self::FormFactory),
            "survey-projects" => Some(Self::SurveyProjects),
            "survey-migration" => Some(Self::SurveyMigration),
            "design-edit" => Some(Self::DesignEdit),
            "design-run" => Some(Self::DesignRun),
            "map" => Some(Self::Map),
            "layers" => Some(Self::Layers),
            "tiling" => Some(Self::Tiling),
            "project" => Some(Self::Project),
            "solar-input" => Some(Self::SolarInput),
            "solar-run" => Some(Self::SolarRun),
            "solar-delivery" => Some(Self::SolarDelivery),
            "operations" => Some(Self::Operations),
            _ => None,
        }
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::AuthContext => "auth-context",
            Self::Grid => "grid",
            Self::GridLocalModel => "grid-local-model",
            Self::Pls => "pls",
            Self::PlsLibrary => "pls-library",
            Self::LibraryGovernance => "library-governance",
            Self::Survey => "survey",
            Self::FormFactory => "form-factory",
            Self::SurveyProjects => "survey-projects",
            Self::SurveyMigration => "survey-migration",
            Self::DesignEdit => "design-edit",
            Self::DesignRun => "design-run",
            Self::Map => "map",
            Self::Layers => "layers",
            Self::Tiling => "tiling",
            Self::Project => "project",
            Self::SolarInput => "solar-input",
            Self::SolarRun => "solar-run",
            Self::SolarDelivery => "solar-delivery",
            Self::Operations => "operations",
        }
    }

    const fn tool_limit(self) -> usize {
        match self {
            // The broad Grid chapter router now also carries the paired
            // application's local model lifecycle and its one project
            // publication. Sixteen leaves plus both bootstrap tools; the
            // narrow `grid-local-model` profile exists for an agent that
            // wants only that workflow.
            Self::Grid => 18,
            // Query, spatial selection, fenced changes, and governed
            // single-entry create belong to the same selected-project Survey
            // workflow. The count includes both bootstrap tools.
            Self::SurveyProjects => 18,
            // Twenty-one governed design-edit leaves plus the two bootstrap
            // tools. Version history and the pinned Working set project the
            // same bounded desktop-owned workflow without transporting
            // features through MCP.
            Self::DesignEdit => 23,
            _ => 16,
        }
    }

    pub fn includes(self, tool: &Tool) -> bool {
        match self {
            Self::AuthContext => AUTH_CONTEXT_COMMANDS.contains(&tool.id.as_str()),
            Self::Grid => matches!(tool.chapter, Chapter::GridModel | Chapter::Reports),
            Self::GridLocalModel => GRID_LOCAL_MODEL_COMMANDS.contains(&tool.id.as_str()),
            Self::Pls => tool.chapter == Chapter::PlsCadd && tool.id.starts_with("pls."),
            Self::PlsLibrary => PLS_LIBRARY_COMMANDS.contains(&tool.id.as_str()),
            Self::LibraryGovernance => LIBRARY_GOVERNANCE_COMMANDS.contains(&tool.id.as_str()),
            Self::Survey => SURVEY_MAP_COMMANDS.contains(&tool.id.as_str()),
            Self::FormFactory => FORM_FACTORY_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyProjects => SURVEY_PROJECT_COMMANDS.contains(&tool.id.as_str()),
            Self::SurveyMigration => SURVEY_MIGRATION_COMMANDS.contains(&tool.id.as_str()),
            Self::Map => tool.chapter == Chapter::MapPresentation,
            Self::Layers => LAYER_COMMANDS.contains(&tool.id.as_str()),
            Self::Tiling => tool.chapter == Chapter::VectorTiles,
            // Native account bootstrap is available on the broad live surface
            // but is not project-workflow tooling and must not inflate the
            // already bounded specialized project profile.
            Self::Project => tool.chapter == Chapter::Project && !tool.id.starts_with("auth."),
            Self::Operations => tool.chapter == Chapter::Operations,
            Self::DesignEdit => DESIGN_EDIT_COMMANDS.contains(&tool.id.as_str()),
            Self::DesignRun => DESIGN_RUN_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarInput => SOLAR_INPUT_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarRun => SOLAR_RUN_COMMANDS.contains(&tool.id.as_str()),
            Self::SolarDelivery => SOLAR_DELIVERY_COMMANDS.contains(&tool.id.as_str()),
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
            Self::GridLocalModel => GRID_LOCAL_MODEL_COMMANDS,
            Self::Survey => SURVEY_MAP_COMMANDS,
            Self::FormFactory => FORM_FACTORY_COMMANDS,
            Self::SurveyProjects => SURVEY_PROJECT_COMMANDS,
            Self::SurveyMigration => SURVEY_MIGRATION_COMMANDS,
            Self::Layers => LAYER_COMMANDS,
            Self::DesignEdit => DESIGN_EDIT_COMMANDS,
            Self::DesignRun => DESIGN_RUN_COMMANDS,
            Self::SolarInput => SOLAR_INPUT_COMMANDS,
            Self::SolarRun => SOLAR_RUN_COMMANDS,
            Self::SolarDelivery => SOLAR_DELIVERY_COMMANDS,
            Self::PlsLibrary => PLS_LIBRARY_COMMANDS,
            Self::LibraryGovernance => LIBRARY_GOVERNANCE_COMMANDS,
            Self::Grid
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
            Self::Grid => matches!(chapter, Chapter::GridModel | Chapter::Reports),
            Self::GridLocalModel => chapter == Chapter::GridModel,
            Self::Pls | Self::PlsLibrary | Self::LibraryGovernance => chapter == Chapter::PlsCadd,
            Self::Survey
            | Self::FormFactory
            | Self::SurveyProjects
            | Self::SurveyMigration
            | Self::Layers => chapter == Chapter::Survey,
            Self::DesignEdit | Self::DesignRun => chapter == Chapter::Design,
            Self::Map => chapter == Chapter::MapPresentation,
            Self::Tiling => chapter == Chapter::VectorTiles,
            Self::Project => chapter == Chapter::Project,
            Self::SolarInput | Self::SolarRun | Self::SolarDelivery => chapter == Chapter::Solar,
            Self::Operations => chapter == Chapter::Operations,
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

// The paired application's DS Grid model lifecycle, in the order the work
// happens. Deliberately narrow: an agent acquiring a model, choosing which one
// occupies Profile and publishing one revision needs these five leaves and
// nothing else, and the four local ones reach no project at all. The one
// project act is published beside them because it is where the workflow ends,
// and it stays confirmation-gated exactly as the CLI declares it.
const GRID_LOCAL_MODEL_COMMANDS: &[&str] = &[
    "dsgrid.model.list",
    "dsgrid.model.create-local",
    "dsgrid.model.import-external",
    "dsgrid.model.set-active",
    "dsgrid.publish-version",
];

const PLS_LIBRARY_COMMANDS: &[&str] = &[
    "library.verify",
    "library.open",
    "library.catalog",
    "library.pack",
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
    "map.layer.reorder",
    "map.layer.remote-list",
    "map.layer.add",
    "map.layer.remove",
    "map.layer.visibility",
];

const FORM_FACTORY_COMMANDS: &[&str] = &[
    "survey.forms.list",
    "survey.form.read",
    "survey.form.types",
    "survey.form.create",
    "survey.form.update",
    "survey.form.lifecycle",
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
    "survey.templates.list",
    "survey.template.read",
    "survey.template.create",
    "survey.template.apply",
    "survey.template.lifecycle",
    "survey.project.create-from-template",
];

// Bulk migration is intentionally isolated from the ordinary Survey project
// profile: one high-blast-radius leaf plus ds_catalog is the whole typed MCP
// surface an import agent receives.
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
    "map.design.version.begin",
    "map.design.version.list",
    "map.design.version.play",
    "map.design.version.compare",
    "map.design.upload.inspect",
    "map.design.upload.stage",
];

const DESIGN_RUN_COMMANDS: &[&str] = &[
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

const SOLAR_INPUT_COMMANDS: &[&str] = &["solar.input.capture", "solar.input.prepare"];

const SOLAR_DELIVERY_COMMANDS: &[&str] = &[
    "solar.portfolio.list",
    "solar.portfolio.read",
    "solar.portfolio.analysis",
    "solar.final.import",
    "solar.final.submit",
    "solar.report.export",
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
        match (self.exposure, self.profile) {
            (Exposure::Chapters, None) => "Use ds_catalog for bounded discovery, call the selected chapter with operation=describe, then operation=invoke. The canonical command descriptor governs arguments, authority, effect, confirmation and refusals; branch on the returned DS envelope.".to_string(),
            (Exposure::Commands, Some(profile)) => format!(
                "This is the typed `{}` profile. Use ds_catalog for bounded discovery, then call the advertised command tool directly. Pass confirm=true only when the command declares it and the user's intent authorizes that exact effect and scope. Branch on the returned DS envelope.",
                profile.token()
            ),
            (Exposure::Commands, None) => "Compatibility command exposure: every advertised tool is one canonical ds command generated from its live descriptor. Pass confirm=true only when declared, branch on the returned DS envelope, and follow typed remedies.".to_string(),
            (Exposure::Chapters, Some(_)) => unreachable!("invalid surface is refused"),
        }
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
        executable: &PathBuf,
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
                "command": live_command_summary(tool, executable)?,
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
                .map(|(_, tool)| live_command_summary(tool, executable))
                .collect::<Result<Vec<_>, _>>()?;
            json!({
                "query": query,
                "matched": matched,
                "results": results,
                "next": "call the matching chapter with operation=describe and the exact command id",
            })
        } else if let Some(chapter) = chapter {
            let commands = visible
                .map(|tool| live_command_summary(tool, executable))
                .collect::<Result<Vec<_>, _>>()?;
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

fn live_command_summary(tool: &Tool, executable: &PathBuf) -> Result<Value, (i64, String)> {
    let descriptor = tools::live_command_descriptor(executable, &tool.id)
        .map_err(|failure| (-32000, failure.to_string()))?;
    Ok(command_summary(tool, &descriptor))
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

fn failure_result(tool: &Tool, failure: &Failure) -> Value {
    let contract = tool.descriptor["contract"].as_u64().unwrap_or(1) as u32;
    let envelope = serde_json::to_value(error_envelope(&tool.id, contract, failure))
        .unwrap_or_else(|_| json!({ "status": "error" }));
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
    let envelope: Option<Value> = serde_json::from_str(stdout.trim()).ok();
    let is_error = code != 0
        || envelope
            .as_ref()
            .and_then(|value| value.get("status"))
            .and_then(Value::as_str)
            != Some("ok");
    let text = if stdout.trim().is_empty() {
        stderr.trim().to_string()
    } else {
        stdout.trim().to_string()
    };
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
            "readOnlyHint": !tool.confirmation_required,
            "openWorldHint": false,
        },
    })
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
                "chapter": { "type": ["string", "null"], "enum": ["project", "grid-model", "pls-cadd", "survey", "design", "map-presentation", "vector-tiles", "solar", "reports", "operations", "workstation", null], "description": "Restrict discovery to one operator-intent chapter." },
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
            "Prepare local data for analysis: inspect a source file, then convert it to the analytical GeoParquet format. Conversion is an explicit step that runs before analysis, never inside it, and needs no project or paired desktop. Describe a command before invoking it."
        }
        Chapter::Project => {
            "Establish project context and manage project plans, tasks, assignments, and records. Describe a command before invoking it."
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
            "Discover report tasks and export or bundle verified report artifacts. Describe a command before invoking it."
        }
        Chapter::Operations => {
            "Inspect platform health, manage shell reachability, and report product gaps. Describe a command before invoking it."
        }
        Chapter::Workstation => {
            "Inspect workstation prerequisites and governed reference components, review mutation-free plans, and verify local evidence. Describe a command before invoking it."
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
            descriptor: json!({ "id": id, "summary": format!("{id} summary"), "availability": "available" }),
        }
    }

    fn conditional_tool(id: &str, chapter: Chapter) -> Tool {
        let mut tool = tool(id, chapter, true);
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
