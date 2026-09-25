//! Command metadata: the single source of truth for dispatch, help,
//! capability discovery and generated reference documentation.
//!
//! Everything a caller can learn about a command is a field here. Help text
//! is rendered from this; the capability inventory is rendered from this;
//! dispatch validates arguments against this. There is no second description
//! of a command anywhere, so help cannot drift from behaviour — a command
//! that gains a flag without declaring it here cannot receive it.
//!
//! The vocabulary for effect and authority is deliberately *not* new. It is
//! the vocabulary `ds-mcp` and the desktop agent bridge already use, so a
//! reader who knows one knows all three. Inventing a fourth set of words for
//! the same three questions — what does this change, who must it prove I am,
//! can it run here — would be the migration's first unforced error.

use std::fmt;

use serde::Serialize;

/// What a command can change. Part of every help screen and every capability
/// descriptor, so blast radius is never inferred from a command's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Effect {
    /// Reads nothing outside this process. Safe with no principal at all.
    Discovery,
    /// Reads authoritative state. Saves, publishes and deletes nothing.
    ReadOnly,
    /// Produces a document a human must apply. Persists nothing, but spends
    /// model credit and reads pinned project context.
    Proposal,
    /// May rotate the native credential and may mutate its fenced context.
    LocalAuthState,
    /// Writes a durable file inside the operator's own workspace and
    /// publishes nothing. Not read-only — the disk changed.
    LocalFileWrite,
    /// Changes only the paired desktop session's visible state.
    LocalUi,
    /// Produces a durable artifact of record.
    ArtifactWrite,
    /// Changes software or user integration settings on this machine.
    MachineWrite,
    /// Mutates shared state through a governed ds-brain contract.
    GlobalWrite,
}

impl Effect {
    /// Every effect class, in blast-radius order.
    pub const ALL: &'static [Self] = &[
        Self::Discovery,
        Self::ReadOnly,
        Self::Proposal,
        Self::LocalAuthState,
        Self::LocalFileWrite,
        Self::LocalUi,
        Self::ArtifactWrite,
        Self::MachineWrite,
        Self::GlobalWrite,
    ];

    /// Parse the token carried by a live command descriptor.
    ///
    /// MCP projects a descriptor's effect into tool annotations. Parsing it
    /// here, beside [`Effect::token`], means the adapter cannot grow a second
    /// effect vocabulary that drifts from this one.
    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|effect| effect.token() == token)
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::ReadOnly => "read_only",
            Self::Proposal => "proposal",
            Self::LocalAuthState => "local_auth_state",
            Self::LocalFileWrite => "local_file_write",
            Self::LocalUi => "local_ui",
            Self::ArtifactWrite => "artifact_write",
            Self::MachineWrite => "machine_write",
            Self::GlobalWrite => "global_write",
        }
    }

    /// Whether invoking this without an explicit human decision would be
    /// wrong. Gates the `--yes` requirement in exactly one place.
    pub const fn needs_confirmation(self) -> bool {
        matches!(
            self,
            Self::ArtifactWrite | Self::MachineWrite | Self::GlobalWrite
        )
    }

    /// One line explaining the class, for command-level help only.
    pub const fn gloss(self) -> &'static str {
        match self {
            Self::Discovery => "reads nothing outside this process",
            Self::ReadOnly => "reads state; writes nothing",
            Self::Proposal => "drafts a document a human must apply",
            Self::LocalAuthState => "changes protected native credential or project state",
            Self::LocalFileWrite => "writes a file in your workspace",
            Self::LocalUi => "changes the paired desktop's visible state",
            Self::ArtifactWrite => "produces a durable artifact of record",
            Self::MachineWrite => "changes software or settings on this machine",
            Self::GlobalWrite => "mutates governed shared state",
        }
    }
}

impl fmt::Display for Effect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

/// What a caller must have proved before a command runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Authority {
    /// No principal, no project, no effect on anyone else's data.
    None,
    /// Requires a paired local desktop bridge. Proves a transport, not a
    /// person: it can never authorize a project API call on its own.
    DesktopPairing,
    /// Requires the paired desktop plus its current signed-in user.
    DesktopUser,
    /// Requires a verified principal and the existing UI project runtime.
    /// The kernel routes a differing CLI target through a verified UI switch;
    /// without a CLI selection the current UI project supplies the target.
    Project,
    /// Requires a restored native user session; never implies a desktop.
    HeadlessUser,
    /// Requires a restored native user and its fenced local project context.
    HeadlessProject,
}

/// Provider-independent authority capabilities.
///
/// Command descriptors retain their historical [`Authority`] tokens. This
/// vocabulary is the normalized authorization seam used to compare those
/// tokens without assuming that a user or project must come from Desktop.
/// `Map` is intentionally not inferred from any legacy token: commands must
/// migrate to an explicit map-aware contract before an interactive viewport
/// can become part of their authority proof.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthorityCapability {
    None,
    User,
    Project,
    Desktop,
    Map,
}

impl AuthorityCapability {
    pub const fn token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::User => "user",
            Self::Project => "project",
            Self::Desktop => "desktop",
            Self::Map => "map",
        }
    }
}

impl fmt::Display for AuthorityCapability {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

const NONE_CAPABILITIES: &[AuthorityCapability] = &[AuthorityCapability::None];
const USER_CAPABILITIES: &[AuthorityCapability] = &[AuthorityCapability::User];
const PROJECT_CAPABILITIES: &[AuthorityCapability] = &[
    AuthorityCapability::User,
    AuthorityCapability::Project,
    AuthorityCapability::Desktop,
];
const HEADLESS_PROJECT_CAPABILITIES: &[AuthorityCapability] =
    &[AuthorityCapability::User, AuthorityCapability::Project];
const DESKTOP_CAPABILITIES: &[AuthorityCapability] = &[AuthorityCapability::Desktop];
const DESKTOP_USER_CAPABILITIES: &[AuthorityCapability] =
    &[AuthorityCapability::User, AuthorityCapability::Desktop];

impl Authority {
    /// Parse the token carried by a live command descriptor.
    ///
    /// MCP uses this when it projects a descriptor into a tool. Keeping the
    /// vocabulary here means the adapter cannot grow a second authority list.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "none" => Some(Self::None),
            "desktop_pairing" => Some(Self::DesktopPairing),
            "desktop_user" => Some(Self::DesktopUser),
            "project" => Some(Self::Project),
            "headless_user" => Some(Self::HeadlessUser),
            "headless_project" => Some(Self::HeadlessProject),
            _ => None,
        }
    }

    /// Normalize a compatibility descriptor into actual required
    /// capabilities without changing the descriptor's public token.
    ///
    /// Legacy `project` remains desktop-backed. Provider-interchangeable
    /// project commands already declare `headless_project`; changing the
    /// legacy meaning requires a separately versioned command migration.
    pub const fn capabilities(self) -> &'static [AuthorityCapability] {
        match self {
            Self::None => NONE_CAPABILITIES,
            Self::DesktopPairing => DESKTOP_CAPABILITIES,
            Self::DesktopUser => DESKTOP_USER_CAPABILITIES,
            Self::Project => PROJECT_CAPABILITIES,
            Self::HeadlessUser => USER_CAPABILITIES,
            Self::HeadlessProject => HEADLESS_PROJECT_CAPABILITIES,
        }
    }

    pub const fn requires_capability(self, capability: AuthorityCapability) -> bool {
        let values = self.capabilities();
        let mut index = 0;
        while index < values.len() {
            if values[index] as u8 == capability as u8 {
                return true;
            }
            index += 1;
        }
        false
    }

    /// Whether this authority needs the paired desktop transport at all.
    pub const fn requires_desktop(self) -> bool {
        self.requires_capability(AuthorityCapability::Desktop)
    }

    /// Whether the paired desktop must report a signed-in user before the
    /// command is sent to its handler.
    pub const fn requires_signed_in_user(self) -> bool {
        self.requires_desktop() && self.requires_capability(AuthorityCapability::User)
    }

    /// Whether a selected desktop project is part of the authority proof.
    pub const fn requires_project(self) -> bool {
        self.requires_desktop() && self.requires_capability(AuthorityCapability::Project)
    }

    pub const fn token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DesktopPairing => "desktop_pairing",
            Self::DesktopUser => "desktop_user",
            Self::Project => "project",
            Self::HeadlessUser => "headless_user",
            Self::HeadlessProject => "headless_project",
        }
    }

    pub const fn gloss(self) -> &'static str {
        match self {
            Self::None => "none — runs offline, signed out",
            Self::DesktopPairing => "a running DS GridDesign session on this machine",
            Self::DesktopUser => "a running DS GridDesign session, signed in",
            Self::Project => "signed in, with a project selected",
            Self::HeadlessUser => "a restored native user session",
            Self::HeadlessProject => "a restored native user with explicit project context",
        }
    }
}

impl fmt::Display for Authority {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

/// Where a command can answer at all: on a bare Server, or only with the
/// paired application window in front of it.
///
/// This is the environment-independent half of the question `availability`
/// cannot carry. `availability` is resolved on the machine in hand and every
/// paired command deliberately answers `available` there, because a
/// descriptor stays useful on a laptop where the application is not running.
/// `requires` is the *declared* fact instead: it does not change when the
/// application starts, stops, or was never installed, so it is the one a
/// caller can trace — "is this command server-first?" — without owning the
/// machine it would run on.
///
/// The desktop is the Server plus a window
/// (`ds-command-kernel/docs/contracts/ds-lens-core-boundary.md` §4 L0a), so
/// [`Requires::Window`] is never a property of a domain, only of the command
/// that has not been given its headless owner yet. Every entry is work the
/// host-transparency backlog still owes; the ceilings in
/// `crates/ds/tests/lens_core_boundary.rs` may only fall.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Requires {
    /// Answers identically on a bare Server and on the desktop. The default,
    /// and what every new command is expected to be.
    Server,
    /// Cannot answer without the running application: its map, its selection,
    /// its open project cache, or an authenticated workflow that still lives
    /// inside the shell.
    Window,
}

impl Requires {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Server => "server",
            Self::Window => "window",
        }
    }

    /// One line for command help. Only the window case is ever printed — a
    /// server command saying "runs on the server" would be noise on every
    /// help screen in the product.
    pub const fn gloss(self) -> &'static str {
        match self {
            Self::Server => "runs headless; no application window needed",
            Self::Window => "needs the paired DS GridDesign window",
        }
    }

    pub const fn is_window(self) -> bool {
        matches!(self, Self::Window)
    }

    /// Parse the token a descriptor carries, so a filter and an MCP
    /// projection read the same vocabulary the JSON prints.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "server" => Some(Self::Server),
            "window" => Some(Self::Window),
            _ => None,
        }
    }

    /// The accepted tokens, in the order a refusal should list them.
    pub const TOKENS: &'static [&'static str] = &["server", "window"];
}

impl fmt::Display for Requires {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

/// How a command answers: inside the invocation, or as a durable job.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    /// Completes before the process exits.
    Sync,
    /// Returns a handle. Nothing synchronous may block on one.
    Job,
}

impl Execution {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Sync => "sync",
            Self::Job => "job",
        }
    }

    /// Parse the token carried by a live command descriptor.
    pub fn from_token(token: &str) -> Option<Self> {
        match token {
            "sync" => Some(Self::Sync),
            "job" => Some(Self::Job),
            _ => None,
        }
    }
}

/// Whether a command can run on this machine right now, and if not, why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    Available,
    /// A concrete missing prerequisite, with the concrete thing to do about
    /// it. A reason without a remedy is a dead end for an agent.
    ///
    /// `code` is the domain's own stable identifier for this prerequisite —
    /// not a generic "unavailable". Dispatch refuses with it verbatim, so a
    /// caller branching on `error.code` sees the same value whether the
    /// prerequisite was caught by the gate or by the command itself.
    Unavailable {
        code: &'static str,
        reason: String,
        remedy: String,
    },
}

impl Availability {
    pub fn unavailable(
        code: &'static str,
        reason: impl Into<String>,
        remedy: impl Into<String>,
    ) -> Self {
        Self::Unavailable {
            code,
            reason: reason.into(),
            remedy: remedy.into(),
        }
    }

    pub const fn token(&self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable { .. } => "unavailable",
        }
    }

    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available)
    }
}

/// How an argument carries its value.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArgKind {
    /// `--name <value>`; also accepts `--name=<value>`.
    Value,
    /// `--name`, present or absent. Never consumes the next token.
    Switch,
    /// `--name <value>` repeated. Order is preserved.
    Repeated,
    /// A bare token, matched by declaration order.
    ///
    /// Used sparingly, and only where the operand *is* the subject of the
    /// command — `ds capabilities network` reads the way a person would say
    /// it, and forcing a flag there would make the most common discovery call
    /// longer for no gain. A command that takes real engineering inputs uses
    /// named flags, so an agent is never guessing what position means.
    Positional,
}

/// One declared input. Anything not declared here is rejected at the door.
#[derive(Debug, Clone, Copy)]
pub struct Arg {
    pub name: &'static str,
    pub kind: ArgKind,
    /// The value placeholder shown in help, e.g. `<path>`. Empty for a switch.
    pub value: &'static str,
    pub required: bool,
    /// The value used when the flag is absent, if any. Stated in help because
    /// an undocumented default is a behaviour an agent has to discover by
    /// experiment.
    pub default: Option<&'static str>,
    /// The closed set of accepted values, when there is one. Enforced, not
    /// merely documented.
    pub choices: &'static [&'static str],
    pub summary: &'static str,
}

impl Arg {
    pub const fn value(
        name: &'static str,
        placeholder: &'static str,
        summary: &'static str,
    ) -> Self {
        Self {
            name,
            kind: ArgKind::Value,
            value: placeholder,
            required: false,
            default: None,
            choices: &[],
            summary,
        }
    }

    pub const fn switch(name: &'static str, summary: &'static str) -> Self {
        Self {
            name,
            kind: ArgKind::Switch,
            value: "",
            required: false,
            default: None,
            choices: &[],
            summary,
        }
    }

    pub const fn positional(
        name: &'static str,
        placeholder: &'static str,
        summary: &'static str,
    ) -> Self {
        Self {
            name,
            kind: ArgKind::Positional,
            value: placeholder,
            required: false,
            default: None,
            choices: &[],
            summary,
        }
    }

    pub const fn repeated(
        name: &'static str,
        placeholder: &'static str,
        summary: &'static str,
    ) -> Self {
        Self {
            name,
            kind: ArgKind::Repeated,
            value: placeholder,
            required: false,
            default: None,
            choices: &[],
            summary,
        }
    }

    pub const fn required(mut self) -> Self {
        self.required = true;
        self
    }

    pub const fn default(mut self, value: &'static str) -> Self {
        self.default = Some(value);
        self
    }

    pub const fn choices(mut self, choices: &'static [&'static str]) -> Self {
        self.choices = choices;
        self
    }
}

/// A runnable example. `command` is executed verbatim by the example test, so
/// an example that stops being true stops the build.
#[derive(Debug, Clone, Copy)]
pub struct Example {
    pub command: &'static str,
    pub note: &'static str,
    /// Whether the test harness may run this. False for an example that needs
    /// a paired desktop, a project, or an operator's own file.
    pub runnable: bool,
}

/// A named way this command declines, and what to do next. Enumerated in help
/// so an agent can plan for failure instead of discovering it.
#[derive(Debug, Clone, Copy)]
pub struct Refusal {
    /// The stable `error.code` emitted. Matches what the handler returns.
    pub code: &'static str,
    pub when: &'static str,
    pub remedy: &'static str,
}

/// The one operator concern a command serves.
///
/// This is a discovery classification, not authority. It is declared beside
/// the canonical command contract so a new command cannot omit it or belong to
/// multiple chapters.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Chapter {
    Catalog,
    /// Local data preparation: inspecting a source and converting it to the
    /// analytical format before any analysis reads it.
    Data,
    Project,
    /// Project Assets: the documents a project holds, in declared and
    /// auto-indexed folders, with previews, classification and links.
    Assets,
    GridModel,
    PlsCadd,
    Survey,
    Design,
    MapPresentation,
    VectorTiles,
    Solar,
    Reports,
    Operations,
    Workstation,
}

impl Chapter {
    pub const ALL: &[Self] = &[
        Self::Catalog,
        Self::Data,
        Self::Project,
        Self::Assets,
        Self::GridModel,
        Self::PlsCadd,
        Self::Survey,
        Self::Design,
        Self::MapPresentation,
        Self::VectorTiles,
        Self::Solar,
        Self::Reports,
        Self::Operations,
        Self::Workstation,
    ];

    pub const fn token(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Data => "data",
            Self::Project => "project",
            Self::Assets => "assets",
            Self::GridModel => "grid-model",
            Self::PlsCadd => "pls-cadd",
            Self::Survey => "survey",
            Self::Design => "design",
            Self::MapPresentation => "map-presentation",
            Self::VectorTiles => "vector-tiles",
            Self::Solar => "solar",
            Self::Reports => "reports",
            Self::Operations => "operations",
            Self::Workstation => "workstation",
        }
    }

    pub fn from_token(token: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|chapter| chapter.token() == token)
    }
}

impl fmt::Display for Chapter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.token())
    }
}

pub struct Command {
    /// Stable dotted id: `dsgrid.inspect`. Used by `ds capabilities <id>`,
    /// by audit records, and as the envelope's `command` field. It never
    /// changes meaning; an incompatible change takes a new id.
    pub id: &'static str,
    /// The invocation path: `["dsgrid", "inspect"]`.
    pub path: &'static [&'static str],
    /// This command's own input/output contract version, independent of the
    /// envelope version and of the binary's release version.
    pub contract: u32,
    /// Stable operator-intent classification used to compress MCP discovery.
    /// Every command declares exactly one chapter; profiles only filter it.
    pub chapter: Chapter,
    /// One line. Appears in domain help and in search results. Keep under 70
    /// characters — every domain index pays for it.
    pub summary: &'static str,
    /// The words a stranger would type looking for this command, when they
    /// are not already in its id or summary.
    ///
    /// This is a *finding aid*, not a second description: `ds capabilities
    /// --search` matches these terms and nothing else reads them, so they
    /// cannot drift from behaviour the way a duplicated sentence would. Each
    /// entry is one lowercase word or a short phrase — the outside name for
    /// what this does (`geoprocessing`, `crs`), a synonym we did not pick
    /// (`overlay` for an intersection), or the tool another stack calls it
    /// (`st_buffer`). `contract.rs` holds this shape.
    ///
    /// Empty is the right answer for a command whose own words already say
    /// what it is.
    pub search: &'static [&'static str],
    /// A short paragraph. Command-level help only; never in an index.
    pub purpose: &'static str,
    pub effect: Effect,
    pub authority: Authority,
    pub execution: Execution,
    pub args: &'static [Arg],
    /// What lands on stdout on success.
    pub output: &'static str,
    pub examples: &'static [Example],
    pub refusals: &'static [Refusal],
    /// Repository-relative path to the deep reference for this command.
    /// Named in help; never inlined into it.
    pub reference: Option<&'static str>,
    /// Where this command can run at all. Declared, not probed: it is the
    /// same answer on every machine, which is what makes it traceable.
    pub requires: Requires,
    /// Resolves availability *without* touching another domain. Cheap: help
    /// and the domain index both call it.
    pub availability: fn() -> Availability,
}

impl Command {
    pub fn arg(&self, name: &str) -> Option<&Arg> {
        self.args.iter().find(|arg| arg.name == name)
    }

    /// The one supported mixed-effect command shape: a machine-level
    /// proposal whose declared boolean `--write` switch selects its writing
    /// path. Global and artifact writes never become conditional merely by
    /// naming an input `write`.
    pub fn confirmation_trigger(&self) -> Option<&'static str> {
        (self.effect == Effect::MachineWrite
            && self
                .arg("write")
                .is_some_and(|arg| arg.kind == ArgKind::Switch))
        .then_some("--write")
    }

    /// The declared previewing path of a writing command: a boolean
    /// `--dry-run` switch. With it set the command writes nothing, so it
    /// needs no confirmation — that is what makes "propose, read the
    /// proposal, confirm with `--yes`" one command rather than two. A
    /// command that only names an input `dry-run` as a value has no such
    /// path.
    pub fn preview_switch(&self) -> Option<&'static str> {
        self.arg("dry-run")
            .is_some_and(|arg| arg.kind == ArgKind::Switch)
            .then_some("--dry-run")
    }

    pub fn confirmation_required_for(&self, inputs: &crate::args::Inputs) -> bool {
        if self.confirmation_trigger().is_some() {
            return inputs.switch("write");
        }
        if self.preview_switch().is_some() && inputs.switch("dry-run") {
            return false;
        }
        self.effect.needs_confirmation()
    }
}

/// A family of commands that share an owner and a vocabulary.
pub struct Domain {
    pub id: &'static str,
    /// One line for the root help screen. Root help is the most expensive
    /// text in the product — every agent reads it. Keep it under 70
    /// characters.
    pub summary: &'static str,
    pub commands: &'static [&'static Command],
}

impl Domain {
    pub fn command(&self, name: &str) -> Option<&'static Command> {
        self.commands
            .iter()
            .copied()
            .find(|command| command.path.last() == Some(&name))
    }
}

#[cfg(test)]
mod tests {
    use super::{
        Arg, Authority, AuthorityCapability, Availability, Chapter, Command, Effect, Execution,
        Requires,
    };

    fn available() -> Availability {
        Availability::Available
    }

    static WRITE_SWITCH: Arg = Arg::switch("write", "Select the writing path.");
    static WRITE_ARGS: &[Arg] = &[WRITE_SWITCH];

    fn confirmation_fixture(effect: Effect) -> Command {
        Command {
            id: "test.confirmation-trigger",
            path: &["test", "confirmation-trigger"],
            contract: 1,
            chapter: Chapter::Catalog,
            summary: "Regression fixture.",
            purpose: "Proves only machine writes can use a conditional write trigger.",
            effect,
            authority: Authority::None,
            execution: Execution::Sync,
            args: WRITE_ARGS,
            output: "Nothing.",
            examples: &[],
            refusals: &[],
            reference: None,
            search: &[],
            requires: Requires::Server,
            availability: available,
        }
    }

    #[test]
    fn chapter_tokens_are_unique_stable_and_round_trip() {
        assert_eq!(Chapter::ALL.len(), 14);
        for (index, chapter) in Chapter::ALL.iter().enumerate() {
            let token = chapter.token();
            assert_eq!(Chapter::from_token(token), Some(*chapter));
            assert_eq!(chapter.to_string(), token);
            assert!(
                !token.is_empty()
                    && token
                        .chars()
                        .all(|value| value.is_ascii_lowercase() || value == '-')
                    && !token.starts_with('-')
                    && !token.ends_with('-')
            );
            assert!(
                Chapter::ALL[..index]
                    .iter()
                    .all(|previous| previous.token() != token),
                "duplicate chapter token `{token}`"
            );
        }
    }

    #[test]
    fn unknown_chapter_tokens_are_not_guessed() {
        assert_eq!(Chapter::from_token("grid_model"), None);
        assert_eq!(Chapter::from_token("Catalog"), None);
        assert_eq!(Chapter::from_token(""), None);
    }

    #[test]
    fn compatibility_authorities_normalize_without_weakening_desktop_proof() {
        use AuthorityCapability::{Desktop, Map, None, Project, User};

        let cases: &[(Authority, &[AuthorityCapability])] = &[
            (Authority::None, &[None]),
            (Authority::HeadlessUser, &[User]),
            (Authority::HeadlessProject, &[User, Project]),
            (Authority::DesktopPairing, &[Desktop]),
            (Authority::DesktopUser, &[User, Desktop]),
            (Authority::Project, &[User, Project, Desktop]),
        ];
        for (authority, expected) in cases {
            assert_eq!(authority.capabilities(), *expected, "{authority}");
            assert!(
                !authority.requires_capability(Map),
                "legacy authority `{authority}` must not invent active-map proof"
            );
        }
        assert!(Authority::Project.requires_desktop());
        assert!(Authority::Project.requires_signed_in_user());
        assert!(Authority::Project.requires_project());
        assert!(!Authority::HeadlessProject.requires_desktop());
    }

    #[test]
    fn effect_and_execution_tokens_round_trip() {
        for effect in Effect::ALL {
            assert_eq!(Effect::from_token(effect.token()), Some(*effect));
        }
        assert_eq!(Effect::ALL.len(), 9, "a new effect class must join `ALL`");
        assert_eq!(Effect::from_token("write"), None);
        for execution in [Execution::Sync, Execution::Job] {
            assert_eq!(Execution::from_token(execution.token()), Some(execution));
        }
        assert_eq!(Execution::from_token("async"), None);
    }

    #[test]
    fn authority_tokens_remain_exactly_backward_compatible() {
        for authority in [
            Authority::None,
            Authority::DesktopPairing,
            Authority::DesktopUser,
            Authority::Project,
            Authority::HeadlessUser,
            Authority::HeadlessProject,
        ] {
            assert_eq!(Authority::from_token(authority.token()), Some(authority));
        }
        assert_eq!(Authority::from_token("user"), None);
        assert_eq!(Authority::from_token("map"), None);
    }

    #[test]
    fn only_machine_write_can_use_the_declared_write_trigger() {
        let machine = confirmation_fixture(Effect::MachineWrite);
        assert_eq!(machine.confirmation_trigger(), Some("--write"));
        assert_eq!(
            crate::help::command_json(&machine)["confirmation_trigger"],
            "--write"
        );
        let preview = crate::parse(&machine, &[]).unwrap();
        assert!(!machine.confirmation_required_for(&preview));
        let write = crate::parse(&machine, &["--write".to_string()]).unwrap();
        assert!(machine.confirmation_required_for(&write));

        for effect in [Effect::GlobalWrite, Effect::ArtifactWrite] {
            let command = confirmation_fixture(effect);
            assert_eq!(command.confirmation_trigger(), None);
            assert!(
                crate::help::command_json(&command)
                    .get("confirmation_trigger")
                    .is_none(),
                "{effect:?} must remain unconditionally gated in its descriptor"
            );
            let absent = crate::parse(&command, &[]).unwrap();
            assert!(command.confirmation_required_for(&absent));
        }
    }

    /// A writing command that declares a `--dry-run` switch has a previewing
    /// path: with the switch set nothing is written, so the confirmation gate
    /// stands aside; without it the gate is exactly what it always was. A
    /// `dry-run` VALUE is not a switch and opens no such path.
    #[test]
    fn a_declared_dry_run_switch_is_the_one_confirmation_free_path_of_a_write() {
        static PREVIEW: Command = Command {
            id: "fixture.propose",
            path: &["fixture", "propose"],
            contract: 1,
            summary: "fixture",
            purpose: "fixture",
            chapter: Chapter::Project,
            effect: Effect::GlobalWrite,
            authority: Authority::None,
            execution: Execution::Sync,
            args: &[Arg::switch(
                "dry-run",
                "Answer the proposal; write nothing.",
            )],
            output: "",
            examples: &[],
            refusals: &[],
            reference: None,
            search: &[],
            requires: Requires::Server,
            availability: available,
        };
        assert_eq!(PREVIEW.preview_switch(), Some("--dry-run"));
        assert_eq!(
            crate::help::command_json(&PREVIEW)["preview_switch"],
            "--dry-run"
        );
        let proposal = crate::parse(&PREVIEW, &["--dry-run".to_string()]).unwrap();
        assert!(!PREVIEW.confirmation_required_for(&proposal));
        let write = crate::parse(&PREVIEW, &[]).unwrap();
        assert!(PREVIEW.confirmation_required_for(&write));

        static VALUE: Command = Command {
            id: "fixture.value",
            path: &["fixture", "value"],
            contract: 1,
            summary: "fixture",
            purpose: "fixture",
            chapter: Chapter::Project,
            effect: Effect::GlobalWrite,
            authority: Authority::None,
            execution: Execution::Sync,
            args: &[Arg::value("dry-run", "<x>", "not a switch")],
            output: "",
            examples: &[],
            refusals: &[],
            reference: None,
            search: &[],
            requires: Requires::Server,
            availability: available,
        };
        assert_eq!(VALUE.preview_switch(), None);
        assert!(
            crate::help::command_json(&VALUE)
                .get("preview_switch")
                .is_none()
        );
        let gated = crate::parse(&VALUE, &["--dry-run".to_string(), "x".to_string()]).unwrap();
        assert!(VALUE.confirmation_required_for(&gated));
    }
}
