//! The command registry: the one place a command becomes reachable.
//!
//! Registering a command is a single act that makes it dispatchable,
//! discoverable, documented and completable at once. There is no second list
//! to keep in step — `ds capabilities` and `ds <domain> --help` walk exactly
//! this table, so a command cannot exist without being described, and cannot
//! be described without existing.
//!
//! Domains are listed in the order the root help prints them. That order is
//! part of the interface: an agent that has seen root help once should find
//! the same domain in the same place next time.

use ds_cli_contract::spec::{Command, Domain};
use ds_cli_contract::{Context, Failure, Handler, Inputs};
use serde_json::Value;

use crate::meta;

/// One registered command: its contract, how to run it, and how to show it
/// to a person. The renderer is separate from the handler so human output is
/// always a projection of the machine result, never a parallel computation.
pub struct Entry {
    pub command: &'static Command,
    pub handler: Handler,
    pub render: fn(&Value) -> String,
}

pub struct Registered {
    pub domain: &'static Domain,
    pub entries: &'static [Entry],
}

/// Every domain, in root-help order. Static because the table is the
/// interface: it is walked by dispatch, by help and by the contract tests,
/// and all three must be looking at the same thing.
static DSGRID_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_dsgrid::inspect::COMMAND,
        handler: ds_cli_dsgrid::inspect::run,
        render: ds_cli_dsgrid::inspect::render,
    },
    Entry {
        command: &ds_cli_dsgrid::validate::COMMAND,
        handler: ds_cli_dsgrid::validate::run,
        render: ds_cli_dsgrid::validate::render,
    },
    Entry {
        command: &ds_cli_dsgrid::describe::COMMAND,
        handler: ds_cli_dsgrid::describe::run,
        render: ds_cli_dsgrid::describe::render,
    },
];

/// The exchange domain lists its commands in the order they are meant to be
/// called: classify, then plan, then convert. Domain help prints this order
/// verbatim, so the index doubles as the procedure — a reader who works down
/// the list is following the safe sequence rather than reconstructing it.
static DSGRID_EXCHANGE_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_dsgrid_exchange::inspect::COMMAND,
        handler: ds_cli_dsgrid_exchange::inspect::run,
        render: ds_cli_dsgrid_exchange::inspect::render,
    },
    Entry {
        command: &ds_cli_dsgrid_exchange::plan::COMMAND,
        handler: ds_cli_dsgrid_exchange::plan::run,
        render: ds_cli_dsgrid_exchange::plan::render,
    },
    Entry {
        command: &ds_cli_dsgrid_exchange::convert::COMMAND,
        handler: ds_cli_dsgrid_exchange::convert::run,
        render: ds_cli_dsgrid_exchange::convert::render,
    },
];

static PLS_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_pls::pole_capacity::COMMAND,
        handler: ds_cli_pls::pole_capacity::run,
        render: ds_cli_pls::pole_capacity::render,
    },
    Entry {
        command: &ds_cli_pls::reference_closure::COMMAND,
        handler: ds_cli_pls::reference_closure::run,
        render: ds_cli_pls::reference_closure::render,
    },
    Entry {
        command: &ds_cli_pls::section_orientation::COMMAND,
        handler: ds_cli_pls::section_orientation::run,
        render: ds_cli_pls::section_orientation::render,
    },
    Entry {
        command: &ds_cli_pls::compare_don::COMMAND,
        handler: ds_cli_pls::compare_don::run,
        render: ds_cli_pls::compare_don::render,
    },
];

static REPORT_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_report::engine::COMMAND,
        handler: ds_cli_report::engine::run,
        render: ds_cli_report::engine::render,
    },
    Entry {
        command: &ds_cli_report::tasks::COMMAND,
        handler: ds_cli_report::tasks::run,
        render: ds_cli_report::tasks::render,
    },
    Entry {
        command: &ds_cli_report::export::COMMAND,
        handler: ds_cli_report::export::run,
        render: ds_cli_report::export::render,
    },
    Entry {
        command: &ds_cli_report::bundle::COMMAND,
        handler: ds_cli_report::bundle::run,
        render: ds_cli_report::bundle::render,
    },
];

static SOLAR_ENTRIES: &[Entry] = &[
    Entry {
        command: &ds_cli_solar::engine::COMMAND,
        handler: ds_cli_solar::engine::run,
        render: ds_cli_solar::engine::render,
    },
    Entry {
        command: &ds_cli_solar::prepare::COMMAND,
        handler: ds_cli_solar::prepare::run,
        render: ds_cli_solar::prepare::render,
    },
    Entry {
        command: &ds_cli_solar::run::COMMAND,
        handler: ds_cli_solar::run::run,
        render: ds_cli_solar::run::render,
    },
    Entry {
        command: &ds_cli_solar::weather::COMMAND,
        handler: ds_cli_solar::weather::run,
        render: ds_cli_solar::weather::render,
    },
];

static DESKTOP_ENTRIES: &[Entry] = &[Entry {
    command: &ds_cli_desktop::status::COMMAND,
    handler: ds_cli_desktop::status::run,
    render: ds_cli_desktop::status::render,
}];

static DOMAINS: &[Registered] = &[
    Registered {
        domain: &ds_cli_dsgrid::DOMAIN,
        entries: DSGRID_ENTRIES,
    },
    Registered {
        domain: &ds_cli_dsgrid_exchange::DOMAIN,
        entries: DSGRID_EXCHANGE_ENTRIES,
    },
    Registered {
        domain: &ds_cli_pls::DOMAIN,
        entries: PLS_ENTRIES,
    },
    Registered {
        domain: &ds_cli_solar::DOMAIN,
        entries: SOLAR_ENTRIES,
    },
    Registered {
        domain: &ds_cli_report::DOMAIN,
        entries: REPORT_ENTRIES,
    },
    Registered {
        domain: &ds_cli_desktop::DOMAIN,
        entries: DESKTOP_ENTRIES,
    },
];

pub fn domains() -> &'static [Registered] {
    DOMAINS
}

/// The root-level commands that describe the CLI itself. They are separate
/// from domains because they are not a subject area — `ds capabilities` is
/// not a kind of engineering work — and because they must stay reachable when
/// every domain is unavailable.
pub fn meta_commands() -> &'static [Entry] {
    meta::ENTRIES
}

pub fn find_domain(id: &str) -> Option<&'static Registered> {
    DOMAINS.iter().find(|registered| registered.domain.id == id)
}

/// Resolve a command from an invocation path, longest match first.
///
/// Returns the entry and how many tokens it consumed, so the caller knows
/// where the command's own arguments begin. Longest-first matters because a
/// domain may hold both `ds solar run` and a future `ds solar run status`:
/// resolving the shorter path first would make the longer one unreachable
/// and silently feed `status` to the wrong command as an operand.
pub fn find_by_path(tokens: &[String]) -> Option<(&'static Entry, usize)> {
    let mut best: Option<(&'static Entry, usize)> = None;
    for registered in DOMAINS {
        for entry in registered.entries {
            let path = entry.command.path;
            if path.len() > tokens.len() {
                continue;
            }
            if !path.iter().zip(tokens).all(|(part, token)| *part == token) {
                continue;
            }
            if best.is_none_or(|(_, consumed)| path.len() > consumed) {
                best = Some((entry, path.len()));
            }
        }
    }
    best
}

/// Resolve a dotted command id such as `dsgrid.inspect`.
pub fn find_by_id(id: &str) -> Option<&'static Entry> {
    DOMAINS
        .iter()
        .flat_map(|registered| registered.entries.iter())
        .chain(meta_commands().iter())
        .find(|entry| entry.command.id == id)
}

/// Every registered command, in registration order. Used by capability search
/// and by the contract tests that hold the whole surface to its rules.
pub fn all_commands() -> Vec<&'static Command> {
    DOMAINS
        .iter()
        .flat_map(|registered| registered.entries.iter())
        .chain(meta_commands().iter())
        .map(|entry| entry.command)
        .collect()
}

/// Run one entry: parse its declared inputs, enforce the confirmation policy
/// its effect class implies, then hand off. Confirmation is checked here, in
/// one place, so a handler cannot forget it.
pub fn dispatch(entry: &Entry, tokens: &[String], context: &Context) -> Result<Value, Failure> {
    let inputs: Inputs = ds_cli_contract::parse(entry.command, tokens)?;

    if entry.command.effect.needs_confirmation() && !context.confirmed {
        return Err(Failure::invalid(
            "confirmation_required",
            format!(
                "`ds {}` {} and needs explicit confirmation",
                entry.command.path.join(" "),
                entry.command.effect.gloss()
            ),
        )
        .remedy("re-run with --yes once you intend the effect")
        .next(format!("ds {} --help", entry.command.path.join(" "))));
    }

    if let ds_cli_contract::Availability::Unavailable {
        code,
        reason,
        remedy,
    } = (entry.command.availability)()
    {
        return Err(Failure::unavailable(code, reason)
            .remedy(remedy)
            .next(format!("ds {} --help", entry.command.path.join(" "))));
    }

    (entry.handler)(&inputs, context)
}
