//! `ds report project compounded` — **deprecated**. The alias that keeps the
//! retired id working for one release.
//!
//! The deliverable is the **Combined Report**, and [`super::combined`] is the
//! command. This id resolves, runs the same implementation, and says in its
//! own receipt that it has been superseded, so a script pinned to the old
//! spelling keeps working and its owner learns the new name from the run
//! rather than from a failure after removal.
//!
//! Nothing here reimplements anything: one `COMMAND` descriptor and two
//! delegations. When the release that carries this alias ships, this file is
//! deleted and only its id disappears.

use ds_cli_contract::outcome::Failure;
use ds_cli_contract::spec::{Authority, Chapter, Command, Effect, Example, Execution, Requires};
use ds_cli_contract::{Context, Inputs};
use serde_json::Value;

pub static COMMAND: Command = Command {
    id: "report.project.compounded",
    path: &["report", "project", "compounded"],
    contract: 1,
    summary: "Deprecated alias for `report project combined`; use that instead.",
    purpose: "\
DEPRECATED. `compounded` was the internal name for the Combined Report; the \
deliverable is now called the Combined Report on every surface. This id runs \
`ds report project combined` unchanged and reports itself deprecated. It is \
kept for one release so nothing in flight breaks; move scripts to `ds report \
project combined`.",
    chapter: Chapter::Reports,
    effect: Effect::ArtifactWrite,
    authority: Authority::HeadlessProject,
    execution: Execution::Sync,
    args: super::combined::ARGS,
    output: super::combined::OUTPUT,
    examples: &[Example {
        command: "ds report project combined --file-level sector --yes --output json",
        note: "The current spelling; this id is the retired one.",
        runnable: false,
    }],
    refusals: super::NATIVE_WRITE_REFUSALS,
    reference: Some("docs/reference/report.md"),
    search: &["combined report"],
    requires: Requires::Server,
    availability: ds_cli_auth::native_availability,
};

pub fn run(inputs: &Inputs, context: &Context) -> Result<Value, Failure> {
    super::combined::execute(inputs, context, Some(COMMAND.id))
}

pub fn render(data: &Value) -> String {
    super::combined::render(data)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The old id must keep RESOLVING — a script that breaks on upgrade is the
    /// rename done wrong — and it must say it is deprecated on every surface a
    /// caller reads, not only in a changelog.
    #[test]
    fn the_retired_id_still_resolves_and_declares_itself_deprecated() {
        assert_eq!(COMMAND.id, "report.project.compounded");
        assert_eq!(COMMAND.path, &["report", "project", "compounded"]);
        assert!(
            COMMAND.summary.starts_with("Deprecated"),
            "{}",
            COMMAND.summary
        );
        assert!(
            COMMAND.purpose.starts_with("DEPRECATED."),
            "{}",
            COMMAND.purpose
        );
        assert!(
            COMMAND.purpose.contains("ds report project combined"),
            "the deprecation must name its replacement: {}",
            COMMAND.purpose
        );
    }

    /// The alias is an alias, not a fork: the same flags, the same output
    /// contract and the same effect class. A second implementation here would
    /// be a second behaviour under a second name, which is what this rename
    /// exists to end.
    #[test]
    fn the_alias_shares_the_combined_contract_exactly() {
        assert_eq!(
            COMMAND.args.len(),
            super::super::combined::COMMAND.args.len()
        );
        assert_eq!(COMMAND.output, super::super::combined::COMMAND.output);
        assert_eq!(COMMAND.effect, super::super::combined::COMMAND.effect);
        assert_eq!(COMMAND.requires, super::super::combined::COMMAND.requires);
        assert_eq!(
            COMMAND.authority.token(),
            super::super::combined::COMMAND.authority.token()
        );
    }

    /// The receipt an alias run produces carries its own deprecation, so a
    /// JSON consumer learns the rename without reading help.
    #[test]
    fn an_alias_receipt_names_the_command_that_supersedes_it() {
        let notice = super::super::combined::deprecation_notice(COMMAND.id);
        assert_eq!(notice["command"], COMMAND.id);
        assert_eq!(notice["superseded_by"], "report.project.combined");
    }
}
