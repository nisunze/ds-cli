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
    /// Writes a durable file inside the operator's own workspace and
    /// publishes nothing. Not read-only — the disk changed.
    LocalFileWrite,
    /// Changes only the paired desktop session's visible state.
    LocalUi,
    /// Produces a durable artifact of record.
    ArtifactWrite,
    /// Mutates shared state through a governed ds-brain contract.
    GlobalWrite,
}

impl Effect {
    pub const fn token(self) -> &'static str {
        match self {
            Self::Discovery => "discovery",
            Self::ReadOnly => "read_only",
            Self::Proposal => "proposal",
            Self::LocalFileWrite => "local_file_write",
            Self::LocalUi => "local_ui",
            Self::ArtifactWrite => "artifact_write",
            Self::GlobalWrite => "global_write",
        }
    }

    /// Whether invoking this without an explicit human decision would be
    /// wrong. Gates the `--yes` requirement in exactly one place.
    pub const fn needs_confirmation(self) -> bool {
        matches!(self, Self::ArtifactWrite | Self::GlobalWrite)
    }

    /// One line explaining the class, for command-level help only.
    pub const fn gloss(self) -> &'static str {
        match self {
            Self::Discovery => "reads nothing outside this process",
            Self::ReadOnly => "reads state; writes nothing",
            Self::Proposal => "drafts a document a human must apply",
            Self::LocalFileWrite => "writes a file in your workspace",
            Self::LocalUi => "changes the paired desktop's visible state",
            Self::ArtifactWrite => "produces a durable artifact of record",
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
    /// Requires a verified principal bound to a confirmed project.
    Project,
}

impl Authority {
    pub const fn token(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::DesktopPairing => "desktop_pairing",
            Self::DesktopUser => "desktop_user",
            Self::Project => "project",
        }
    }

    pub const fn gloss(self) -> &'static str {
        match self {
            Self::None => "none — runs offline, signed out",
            Self::DesktopPairing => "a running DS GridDesign session on this machine",
            Self::DesktopUser => "a running DS GridDesign session, signed in",
            Self::Project => "signed in, with a project selected",
        }
    }
}

impl fmt::Display for Authority {
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
    Project,
    GridModel,
    PlsCadd,
    Survey,
    Design,
    MapPresentation,
    VectorTiles,
    Solar,
    Reports,
    Operations,
}

impl Chapter {
    pub const ALL: &[Self] = &[
        Self::Catalog,
        Self::Project,
        Self::GridModel,
        Self::PlsCadd,
        Self::Survey,
        Self::Design,
        Self::MapPresentation,
        Self::VectorTiles,
        Self::Solar,
        Self::Reports,
        Self::Operations,
    ];

    pub const fn token(self) -> &'static str {
        match self {
            Self::Catalog => "catalog",
            Self::Project => "project",
            Self::GridModel => "grid-model",
            Self::PlsCadd => "pls-cadd",
            Self::Survey => "survey",
            Self::Design => "design",
            Self::MapPresentation => "map-presentation",
            Self::VectorTiles => "vector-tiles",
            Self::Solar => "solar",
            Self::Reports => "reports",
            Self::Operations => "operations",
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
    /// Resolves availability *without* touching another domain. Cheap: help
    /// and the domain index both call it.
    pub availability: fn() -> Availability,
}

impl Command {
    pub fn arg(&self, name: &str) -> Option<&Arg> {
        self.args.iter().find(|arg| arg.name == name)
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
    use super::Chapter;

    #[test]
    fn chapter_tokens_are_unique_stable_and_round_trip() {
        assert_eq!(Chapter::ALL.len(), 11);
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
}
