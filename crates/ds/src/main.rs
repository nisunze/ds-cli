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
struct Globals {
    output: Output,
    confirmed: bool,
    help: bool,
    version: bool,
}

fn split_globals(argv: &[String]) -> Result<(Globals, Vec<String>), Failure> {
    let mut rest: Vec<String> = Vec::new();
    let mut format = Format::Human;
    let mut pretty = false;
    let mut no_color = false;
    let mut confirmed = false;
    let mut help = false;
    let mut version = false;

    let mut index = 0usize;
    while index < argv.len() {
        let token = &argv[index];
        index += 1;
        match token.as_str() {
            "--help" | "-h" => help = true,
            "--version" | "-V" => version = true,
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
            version,
        },
        rest,
    ))
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

    if globals.version {
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
                    emit_failure(output, "ds", 1, &unknown_command(registered.domain, two))
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
