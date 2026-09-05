//! `ds` — the Data Solutions command line.
//!
//! One executable is the door into the whole stack, for a person in a
//! terminal and for a coding agent that has never seen it before. That only
//! works if discovery is tiered: root help names domains, domain help names
//! commands, command help is one complete contract. Nothing prints the tier
//! below it, so a caller interested in one domain never pays the context cost
//! of the rest.
//!
//! Routing happens here and is deliberately shallow:
//!
//!   ds                        root help
//!   ds <meta> [...]           capabilities · doctor · version
//!   ds <domain>               domain help
//!   ds <domain> <command> …   parse against the command's declaration, run
//!
//! `--help` is intercepted at whatever depth it appears, before parsing, so
//! asking for help is never itself an invalid invocation.

mod build;
mod meta;
mod registry;

use std::process::ExitCode;

use ds_cli_contract::outcome::{ExitClass, Failure};
use ds_cli_contract::output::{Format, Output};
use ds_cli_contract::spec::Domain;
use ds_cli_contract::{Context, help};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    match run(&argv) {
        Ok(()) => ExitCode::SUCCESS,
        Err((class, ())) => ExitCode::from(class.code()),
    }
}

/// Global flags, stripped before routing so they may appear anywhere. They
/// are the only flags with that privilege; everything else belongs to exactly
/// one command and is validated against its declaration.
///
/// "Anywhere" stops at `--`. Without an end-of-options sentinel this scan
/// reads every token, including one a command meant as an operand, so a
/// caller passing the literal text `--yes` as an operand would have silently
/// granted global confirmation on the way past. The live `capabilities`
/// command already declares the optional `selector` operand, so the sentinel
/// is a current boundary, not speculative parser hardening.
///
/// "Anywhere" also stops at a command that declares its own `--version`.
/// That flag is the one global whose name a command legitimately owns —
/// `ds design comment post --version <version-id>` pins an object version —
/// and stripping it here read the write as a request for the binary's
/// version, printed the release envelope and exited 0 without posting
/// anything. [`version_is_global`] makes that decision after the command is
/// known instead of before, so the space and `=` forms behave identically and
/// a declared input is never shadowed. See [`Globals::version`].
struct Globals {
    output: Output,
    confirmed: bool,
    help: bool,
}

fn split_globals(argv: &[String]) -> Result<(Globals, Vec<String>), Failure> {
    let mut rest: Vec<String> = Vec::new();
    let mut format = Format::Human;
    let mut pretty = false;
    let mut no_color = false;
    let mut confirmed = false;
    let mut help = false;

    let mut index = 0usize;
    while index < argv.len() {
        let token = &argv[index];
        index += 1;
        match token.as_str() {
            // Everything past the sentinel belongs to the command, verbatim.
            // The sentinel itself travels with it so argument parsing sees the
            // same boundary this scan did.
            "--" => {
                rest.extend_from_slice(&argv[index - 1..]);
                break;
            }
            "--help" | "-h" => help = true,
            "--pretty" => pretty = true,
            "--no-color" => no_color = true,
            "--yes" => confirmed = true,
            "--output" => {
                let value = argv.get(index).ok_or_else(|| {
                    Failure::invalid("missing_value", "`--output` needs a value")
                        .remedy("pass `--output human` or `--output json`")
                })?;
                index += 1;
                format = parse_format(value)?;
            }
            other => {
                if let Some(value) = other.strip_prefix("--output=") {
                    format = parse_format(value)?;
                } else {
                    rest.push(token.clone());
                }
            }
        }
    }

    Ok((
        Globals {
            output: Output::resolve(format, pretty, no_color),
            confirmed,
            help,
        },
        rest,
    ))
}

/// Whether `--version` in this invocation is the request for the binary's own
/// version, rather than an input belonging to the command that was named.
///
/// The decision needs the command, so it happens after routing tokens are
/// known and never inside [`split_globals`]. Two rules, and both matter:
///
/// * If the resolved command declares an input named `version`, the flag is
///   that command's — in every spelling. `--version v3`, `--version=v3` and
///   `-V v3` all reach the command's own parser, which reports a missing or
///   unknown value with a code the command documents. A write can no longer
///   answer with the release envelope and exit 0 having changed nothing.
/// * Otherwise the flag keeps its old reach: it is global at any depth, so
///   `ds dsgrid inspect --version` still reports the binary.
///
/// The scan stops at `--` for the same reason [`split_globals`] does: past
/// the sentinel every token is an operand, including one spelled `--version`.
fn version_is_global(rest: &[String]) -> bool {
    if command_declares_version(rest) {
        return false;
    }
    rest.iter()
        .take_while(|token| token.as_str() != "--")
        .any(|token| matches!(token.as_str(), "--version" | "-V"))
}

/// Whether the command these tokens name declares its own `version` input.
///
/// Read from the live declaration rather than a list kept here: a command
/// that gains or loses `--version` changes this answer in the same commit,
/// and there is no second place to update.
fn command_declares_version(rest: &[String]) -> bool {
    let declares = |command: &'static ds_cli_contract::Command| command.arg("version").is_some();
    if let Some((entry, _)) = registry::find_by_path(rest) {
        return declares(entry.command);
    }
    rest.first().is_some_and(|first| {
        registry::meta_commands()
            .iter()
            .any(|entry| entry.command.path == [first.as_str()] && declares(entry.command))
    })
}

fn parse_format(value: &str) -> Result<Format, Failure> {
    match value {
        "human" => Ok(Format::Human),
        "json" => Ok(Format::Json),
        _ => Err(
            Failure::invalid("invalid_choice", "`--output` does not accept that value")
                .remedy("use `--output human` or `--output json`")
                .detail(serde_json::json!({ "accepted": ["human", "json"] })),
        ),
    }
}

fn run(argv: &[String]) -> Result<(), (ExitClass, ())> {
    // Global parsing can itself fail, and it fails before any command is
    // known — so the envelope names `ds` rather than inventing a command.
    let (globals, rest) = match split_globals(argv) {
        Ok(parsed) => parsed,
        Err(failure) => {
            let output = Output::resolve(Format::Human, false, false);
            return emit_failure(output, "ds", 1, &failure);
        }
    };

    let domains: Vec<&'static Domain> = registry::domains()
        .iter()
        .map(|registered| registered.domain)
        .collect();

    if version_is_global(&rest) {
        return finish(globals.output, &meta::VERSION, build::identity(), |data| {
            meta_render(&meta::VERSION, data)
        });
    }

    // Bare `ds`, or `ds --help`: the map. Exit 0 — an agent that runs the
    // bare command asked where it is, and got a correct answer.
    let Some(first) = rest.first() else {
        return globals
            .output
            .text(&help::root(build::PRODUCT, &domains))
            .map_err(|_| (ExitClass::Internal, ()));
    };

    // `ds help <path...>` is the same as `--help` at that path. Agents reach
    // for both spellings; neither should be the wrong guess.
    if first == "help" {
        let path: Vec<String> = rest[1..].to_vec();
        return show_help(globals.output, &domains, &path);
    }

    if globals.help && rest.len() <= 2 {
        return show_help(globals.output, &domains, &rest);
    }

    // Meta commands live at the root because they describe the CLI itself and
    // must stay reachable when every domain is unavailable.
    if let Some(entry) = registry::meta_commands()
        .iter()
        .find(|entry| entry.command.path == [first.as_str()])
    {
        if globals.help {
            return show_command_help(globals.output, entry.command);
        }
        let context = Context {
            confirmed: globals.confirmed,
            output: globals.output,
        };
        return match registry::dispatch(entry, &rest[1..], &context) {
            Ok(data) => finish(globals.output, entry.command, data, entry.render),
            Err(failure) => emit_failure(
                globals.output,
                entry.command.id,
                entry.command.contract,
                &failure,
            ),
        };
    }

    let Some(registered) = registry::find_domain(first) else {
        return emit_failure(globals.output, "ds", 1, &unknown_domain(first, &domains));
    };

    // `ds <domain>` with nothing after it is a question, not a mistake.
    let Some(second) = rest.get(1) else {
        return globals
            .output
            .text(&help::domain(registered.domain))
            .map_err(|_| (ExitClass::Internal, ()));
    };

    let Some((entry, consumed)) = registry::find_by_path(&rest) else {
        if registered.domain.commands.iter().any(|command| {
            command.path.len() > rest.len() && command.path.iter().zip(&rest).all(|(a, b)| *a == b)
        }) {
            return show_help(globals.output, &domains, &rest);
        }
        return emit_failure(
            globals.output,
            "ds",
            1,
            &unknown_command(registered.domain, second),
        );
    };

    if globals.help {
        return show_command_help(globals.output, entry.command);
    }

    let context = Context {
        confirmed: globals.confirmed,
        output: globals.output,
    };
    match registry::dispatch(entry, &rest[consumed..], &context) {
        Ok(data) => finish(globals.output, entry.command, data, entry.render),
        Err(failure) => emit_failure(
            globals.output,
            entry.command.id,
            entry.command.contract,
            &failure,
        ),
    }
}

/// Help at whatever depth was named. In JSON mode a command's help is its
/// machine descriptor — the same facts from the same source, so a caller that
/// wants a schema never parses a help screen.
fn show_help(
    output: Output,
    domains: &[&'static Domain],
    path: &[String],
) -> Result<(), (ExitClass, ())> {
    match path {
        [] => output
            .text(&help::root(build::PRODUCT, domains))
            .map_err(|_| (ExitClass::Internal, ())),
        [one] => {
            if let Some(entry) = registry::meta_commands()
                .iter()
                .find(|entry| entry.command.path == [one.as_str()])
            {
                return show_command_help(output, entry.command);
            }
            match registry::find_domain(one) {
                Some(registered) => output
                    .text(&help::domain(registered.domain))
                    .map_err(|_| (ExitClass::Internal, ())),
                None => emit_failure(output, "ds", 1, &unknown_domain(one, domains)),
            }
        }
        [one, two, ..] => match registry::find_by_path(path) {
            Some((entry, _)) => show_command_help(output, entry.command),
            None => match registry::find_domain(one) {
                Some(registered) => {
                    let commands: Vec<_> = registered.domain.commands.iter().filter(|command| command.path.len() > path.len() && command.path.iter().zip(path).all(|(a,b)| *a == b)).map(|command| serde_json::json!({"id":command.id,"path":command.path,"summary":command.summary})).collect();
                    if commands.is_empty() {
                        return emit_failure(
                            output,
                            "ds",
                            1,
                            &unknown_command(registered.domain, two),
                        );
                    }
                    output
                        .success(
                            "ds.help",
                            1,
                            serde_json::json!({"path":path,"commands":commands}),
                            |data| {
                                let prefix = data["path"].as_array().unwrap();
                                let mut text = format!(
                                    "ds {}\n\nCOMMANDS\n",
                                    prefix
                                        .iter()
                                        .filter_map(|v| v.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" ")
                                );
                                for command in data["commands"].as_array().unwrap() {
                                    let leaf = command["path"]
                                        .as_array()
                                        .unwrap()
                                        .iter()
                                        .skip(prefix.len())
                                        .filter_map(|v| v.as_str())
                                        .collect::<Vec<_>>()
                                        .join(" ");
                                    text.push_str(&format!(
                                        "  {leaf}  {}\n",
                                        command["summary"].as_str().unwrap()
                                    ));
                                }
                                text.push_str(
                                    "\nAppend --help to a command for its exact inputs.\n",
                                );
                                text
                            },
                        )
                        .map_err(|_| (ExitClass::Internal, ()))
                }
                None => emit_failure(output, "ds", 1, &unknown_domain(one, domains)),
            },
        },
    }
}

fn show_command_help(
    output: Output,
    command: &'static ds_cli_contract::Command,
) -> Result<(), (ExitClass, ())> {
    if output.is_json() {
        return output
            .success(
                command.id,
                command.contract,
                help::command_json(command),
                |_| String::new(),
            )
            .map_err(|_| (ExitClass::Internal, ()));
    }
    output
        .text(&help::command(command))
        .map_err(|_| (ExitClass::Internal, ()))
}

fn finish(
    output: Output,
    command: &'static ds_cli_contract::Command,
    data: serde_json::Value,
    render: fn(&serde_json::Value) -> String,
) -> Result<(), (ExitClass, ())> {
    output
        .success(command.id, command.contract, data, render)
        .map_err(|_| (ExitClass::Internal, ()))
}

fn meta_render(_command: &'static ds_cli_contract::Command, data: &serde_json::Value) -> String {
    format!(
        "{} {}  {}{}",
        data["product"].as_str().unwrap_or("ds"),
        data["version"].as_str().unwrap_or(""),
        data["source_sha"].as_str().unwrap_or("unknown"),
        if data["dirty"].as_bool().unwrap_or(false) {
            " (dirty)"
        } else {
            ""
        },
    )
}

fn emit_failure(
    output: Output,
    command: &str,
    contract: u32,
    failure: &Failure,
) -> Result<(), (ExitClass, ())> {
    let _ = output.failure(command, contract, failure);
    Err((failure.class(), ()))
}

fn unknown_domain(name: &str, domains: &[&'static Domain]) -> Failure {
    let mut failure = Failure::invalid(
        "unknown_domain",
        format!("`{name}` is not a ds domain or command"),
    );
    let meta_ids = registry::meta_commands()
        .iter()
        .map(|entry| entry.command.id);
    let candidates = domains.iter().map(|domain| domain.id).chain(meta_ids);
    match ds_cli_contract::args::nearest(name, candidates) {
        Some(suggestion) => failure = failure.remedy(format!("did you mean `ds {suggestion}`?")),
        None => failure = failure.remedy("run `ds --help` for the domain list"),
    }
    failure.next("ds --help").detail(serde_json::json!({
        "domains": domains.iter().map(|domain| domain.id).collect::<Vec<_>>(),
    }))
}

fn unknown_command(domain: &'static Domain, name: &str) -> Failure {
    // The first token after the domain, which is what the caller got wrong.
    // For a nested command that is its group name, not its leaf — suggesting
    // the leaf would send them to a path that does not start where they are.
    let mut names: Vec<&str> = domain
        .commands
        .iter()
        .filter_map(|command| command.path.get(1).copied())
        .collect();
    names.dedup();
    let mut failure = Failure::invalid(
        "unknown_command",
        format!("`{name}` is not a command of `ds {}`", domain.id),
    );
    match ds_cli_contract::args::nearest(name, names.iter().copied()) {
        Some(suggestion) => {
            failure = failure.remedy(format!("did you mean `ds {} {suggestion}`?", domain.id));
        }
        None => failure = failure.remedy(format!("run `ds {} --help`", domain.id)),
    }
    failure
        .next(format!("ds {} --help", domain.id))
        .detail(serde_json::json!({ "commands": names }))
}

#[cfg(test)]
mod tests {
    use super::{split_globals, version_is_global};
    use crate::{meta, registry};
    use ds_cli_contract::args::parse;
    use ds_cli_contract::spec::{ArgKind, Effect};

    fn argv(parts: &[&str]) -> Vec<String> {
        parts.iter().map(|part| (*part).to_string()).collect()
    }

    /// Every registered command that declares its own `--version` input.
    /// Walked from the live surface so a command that gains one is covered
    /// the day it lands, not the day someone remembers this test.
    fn commands_declaring_version() -> Vec<&'static ds_cli_contract::Command> {
        registry::all_commands()
            .into_iter()
            .filter(|command| command.arg("version").is_some())
            .collect()
    }

    #[test]
    fn a_command_that_owns_version_is_never_shadowed_by_the_global_one() {
        // F: `ds design comment post --version v3 …` stripped `--version` on
        // the way past, printed the release envelope and exited 0 without
        // posting. The flag belongs to whichever command declared it, and the
        // decision has to happen after the command is known.
        let declaring = commands_declaring_version();
        assert!(
            !declaring.is_empty(),
            "no command declares `--version`; this guard has stopped covering anything"
        );
        for command in declaring {
            let mut path = command.path.to_vec();
            path.push("--version");
            let (_, rest) = split_globals(&argv(&path)).expect("split");
            assert!(
                !version_is_global(&rest),
                "`ds {}` declares `--version` but the global flag still claimed it",
                command.path.join(" ")
            );
            assert!(
                rest.contains(&"--version".to_string()),
                "`ds {}` must receive its own `--version` token",
                command.path.join(" ")
            );
        }
    }

    #[test]
    fn every_spelling_of_a_command_scoped_version_reaches_the_same_parser() {
        // Space, `=` and the short form must not disagree. The `=` form
        // already fell through to the command; the space form did not, so one
        // spelling wrote and the other reported the binary's version.
        let command = commands_declaring_version()
            .into_iter()
            .next()
            .expect("a command declares --version");
        let base = command.path.to_vec();
        for tail in [
            vec!["--version", "v3"],
            vec!["--version=v3"],
            vec!["-V", "v3"],
        ] {
            let mut invocation = base.clone();
            invocation.extend_from_slice(&tail);
            let (_, rest) = split_globals(&argv(&invocation)).expect("split");
            assert!(
                !version_is_global(&rest),
                "`ds {}` answered with the global version for `{}`",
                command.path.join(" "),
                tail.join(" ")
            );
        }
    }

    #[test]
    fn no_write_can_answer_a_version_request_with_the_release_envelope() {
        // The consequence worth naming: a command that changes durable state
        // must never be reachable by a token that silently turns it into a
        // read of the binary's own version.
        for command in commands_declaring_version() {
            if !matches!(
                command.effect,
                Effect::ArtifactWrite | Effect::GlobalWrite | Effect::MachineWrite
            ) {
                continue;
            }
            for tail in [
                vec!["--version", "v3"],
                vec!["--version=v3"],
                vec!["--version"],
                vec!["-V"],
            ] {
                let mut invocation = command.path.to_vec();
                invocation.extend_from_slice(&tail);
                let (_, rest) = split_globals(&argv(&invocation)).expect("split");
                assert!(
                    !version_is_global(&rest),
                    "the write `ds {}` returns the global version envelope for `{}`",
                    command.path.join(" "),
                    tail.join(" ")
                );
            }
        }
    }

    #[test]
    fn the_global_version_flag_keeps_its_reach_where_no_command_claims_it() {
        for invocation in [
            vec!["--version"],
            vec!["-V"],
            vec!["--version", "--output", "json"],
            // A command with no `version` input of its own: unchanged.
            vec!["dsgrid", "inspect", "--version"],
        ] {
            let (_, rest) = split_globals(&argv(&invocation)).expect("split");
            assert!(
                version_is_global(&rest),
                "`ds {}` no longer reports the binary's version",
                invocation.join(" ")
            );
        }
    }

    #[test]
    fn a_version_token_after_the_sentinel_is_an_operand_not_the_binarys_version() {
        let (_, rest) = split_globals(&argv(&["capabilities", "--", "--version"])).expect("split");
        assert!(!version_is_global(&rest));
        let inputs = parse(&meta::CAPABILITIES, &rest[1..]).expect("real selector parses");
        assert_eq!(inputs.value("selector"), Some("--version"));
    }

    #[test]
    fn globals_are_still_recognised_at_any_depth() {
        let (globals, rest) = split_globals(&argv(&["dsgrid", "inspect", "--yes"])).expect("split");
        assert!(globals.confirmed);
        assert_eq!(rest, argv(&["dsgrid", "inspect"]));
    }

    #[test]
    fn a_token_after_the_sentinel_never_becomes_global_confirmation() {
        // F8: capabilities has a real positional `selector`; literal --yes
        // after -- must be that selector, never global confirmation.
        let (globals, rest) =
            split_globals(&argv(&["capabilities", "--", "--yes"])).expect("split");
        assert!(
            !globals.confirmed,
            "a value that reads like `--yes` must never confirm anything"
        );
        assert_eq!(rest, argv(&["capabilities", "--", "--yes"]));
        let inputs = parse(&meta::CAPABILITIES, &rest[1..]).expect("real selector parses");
        assert_eq!(inputs.value("selector"), Some("--yes"));
    }

    #[test]
    fn the_sentinel_shields_every_global_not_just_confirmation() {
        let (globals, rest) = split_globals(&argv(&[
            "work", "task", "create", "--", "--help", "--output", "json",
        ]))
        .expect("split");
        assert!(!globals.help);
        assert!(!globals.output.is_json());
        assert_eq!(
            rest,
            argv(&["work", "task", "create", "--", "--help", "--output", "json"])
        );
    }

    #[test]
    fn globals_before_the_sentinel_are_still_honoured() {
        let (globals, rest) =
            split_globals(&argv(&["--output", "json", "capabilities", "--", "--yes"]))
                .expect("split");
        assert!(globals.output.is_json());
        assert!(!globals.confirmed);
        assert_eq!(rest, argv(&["capabilities", "--", "--yes"]));
    }

    #[test]
    fn the_sentinel_travels_with_the_command_so_parsing_sees_the_same_boundary() {
        // `find_by_path` matches path segments, so `--` cannot be mistaken for
        // one; it reaches `ds_cli_contract::parse`, which owns operands.
        let (_, rest) = split_globals(&argv(&["style", "list", "--", "-x"])).expect("split");
        assert!(rest.contains(&"--".to_string()));
    }

    #[test]
    fn confirmation_gated_commands_have_no_positional_boundary_without_a_reviewed_test() {
        let commands = registry::meta_commands()
            .iter()
            .map(|entry| entry.command)
            .chain(
                registry::domains()
                    .iter()
                    .flat_map(|domain| domain.domain.commands.iter().copied()),
            );
        for command in commands {
            if command.effect.needs_confirmation() {
                assert!(
                    command
                        .args
                        .iter()
                        .all(|arg| arg.kind != ArgKind::Positional),
                    "confirmation-gated command `{}` gained a positional input; add a reviewed --/--yes boundary test before allowing it",
                    command.id
                );
            }
        }
    }
}
