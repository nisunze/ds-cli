//! Tiered help rendering.
//!
//! Context is a testable interface budget. The principal consumers are coding
//! agents whose entire working memory is spent on whatever `ds` prints, so
//! every tier prints exactly one thing:
//!
//!   tier 1  root      identity, domain one-liners, how to drill down
//!   tier 2  domain    that domain's command index, one line each
//!   tier 3  command   one complete contract
//!   tier 4  reference a path to a versioned document, never inlined
//!
//! No tier prints the tier below it. A caller looking for a solar command
//! must never pay for the PLS, QGIS or reporting contracts, and adding a
//! domain must cost root help exactly one line. `tests/context_budget.rs`
//! asserts both, in bytes.

use std::fmt::Write as _;

use serde_json::{Value, json};

use crate::spec::{Arg, ArgKind, Command, Domain};

/// Global flags. Printed once at the root and never repeated per command:
/// repeating them would multiply the most-read text in the product by the
/// number of commands.
pub const GLOBAL_FLAGS: &[(&str, &str)] = &[
    ("--output human|json", "stdout format (default: human)"),
    ("--pretty", "indent JSON; costs bytes, aids humans"),
    ("--no-color", "never emit ANSI (also honours NO_COLOR)"),
    ("--yes", "pre-confirm an effectful command"),
    ("--version", "build identity"),
];

/// Tier 1. The most expensive text in the product — every agent reads it, and
/// it is the only screen that does not answer a specific question.
pub fn root(product: &str, domains: &[&'static Domain]) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "{product} — the Data Solutions command line.\n");
    let _ = writeln!(out, "USAGE\n  ds <domain> <command> [--flags]\n");

    let _ = writeln!(out, "DOMAINS");
    let width = domains.iter().map(|d| d.id.len()).max().unwrap_or(0);
    for domain in domains {
        let _ = writeln!(out, "  {:<width$}  {}", domain.id, domain.summary);
    }

    let _ = writeln!(
        out,
        "\nDISCOVERY\n  \
         ds <domain> --help             commands in one domain\n  \
         ds <domain> <cmd> --help       one command's full contract\n  \
         ds capabilities [<domain>|<id>]  the same, as JSON\n  \
         ds capabilities --search <text>  find a command by words\n  \
         ds doctor                      what works here, and why not"
    );

    let _ = writeln!(out, "\nGLOBAL");
    let width = GLOBAL_FLAGS
        .iter()
        .map(|(flag, _)| flag.len())
        .max()
        .unwrap_or(0);
    for (flag, summary) in GLOBAL_FLAGS {
        let _ = writeln!(out, "  {flag:<width$}  {summary}");
    }

    let _ = writeln!(
        out,
        "\nMachine output is stdout; diagnostics are stderr.\n\
         Exit  0 ok · 2 input · 3 unavailable · 4 auth · 5 conflict · 6 failed · 1 bug"
    );
    out
}

/// Tier 2. One domain's index. Availability is resolved here because "which
/// of these can I actually run" is the question a caller has at exactly this
/// moment — but only for *this* domain's commands.
pub fn domain(domain: &Domain) -> String {
    let mut out = String::new();
    let _ = writeln!(out, "ds {} — {}\n", domain.id, domain.summary);
    let _ = writeln!(out, "COMMANDS");

    let leaves: Vec<(String, &'static Command)> = domain
        .commands
        .iter()
        .map(|command| (command.path[1..].join(" "), *command))
        .collect();
    let width = leaves.iter().map(|(path, _)| path.len()).max().unwrap_or(0);

    for (path, command) in &leaves {
        let availability = (command.availability)();
        let mark = if availability.is_available() {
            ""
        } else {
            "  [unavailable]"
        };
        let _ = writeln!(out, "  {path:<width$}  {}{mark}", command.summary);
    }

    // Both hint lines are padded to a common column so the domain's name,
    // whatever its length, does not leave the two forms ragged.
    let drill = format!("ds {} <command> --help", domain.id);
    let machine = format!("ds capabilities {}", domain.id);
    let width = drill.len().max(machine.len());
    let _ = writeln!(
        out,
        "\n  {drill:<width$}   full contract, inputs, refusals\n  \
         {machine:<width$}   the same index, as JSON"
    );
    out
}

/// Tier 3. One command's complete contract: everything needed to invoke it
/// correctly and to handle its failures, and nothing about any other command.
pub fn command(command: &Command) -> String {
    let mut out = String::new();
    let path = command.path.join(" ");

    let _ = writeln!(out, "ds {path} — {}\n", command.summary);
    let _ = writeln!(out, "{}\n", wrap(command.purpose, 76, "  "));

    let _ = writeln!(out, "USAGE\n  ds {path}{}\n", usage_tail(command.args));

    let _ = writeln!(
        out,
        "CONTRACT\n  \
         effect     {}  ({})\n  \
         authority  {}  ({})\n  \
         execution  {}\n  \
         id         {}   contract v{}",
        command.effect,
        command.effect.gloss(),
        command.authority,
        command.authority.gloss(),
        command.execution.token(),
        command.id,
        command.contract,
    );

    if !command.args.is_empty() {
        let _ = writeln!(out, "\nINPUTS");
        let rendered: Vec<(String, &Arg)> = command
            .args
            .iter()
            .map(|arg| (flag_form(arg), arg))
            .collect();
        let width = rendered
            .iter()
            .map(|(form, _)| form.len())
            .max()
            .unwrap_or(0);
        for (form, arg) in &rendered {
            let mut tail = String::new();
            if arg.required {
                tail.push_str("  (required)");
            }
            if let Some(default) = arg.default {
                let _ = write!(tail, "  (default: {default})");
            }
            if !arg.choices.is_empty() {
                let _ = write!(tail, "  (one of: {})", arg.choices.join(", "));
            }
            let _ = writeln!(out, "  {form:<width$}  {}{tail}", arg.summary);
        }
    }

    let _ = writeln!(out, "\nOUTPUT\n{}", wrap(command.output, 76, "  "));

    if !command.examples.is_empty() {
        let _ = writeln!(out, "\nEXAMPLES");
        for example in command.examples {
            let _ = writeln!(out, "  {}", example.command);
            if !example.note.is_empty() {
                let _ = writeln!(out, "      {}", example.note);
            }
        }
    }

    if !command.refusals.is_empty() {
        let _ = writeln!(out, "\nREFUSALS");
        let width = command
            .refusals
            .iter()
            .map(|r| r.code.len())
            .max()
            .unwrap_or(0);
        for refusal in command.refusals {
            let _ = writeln!(out, "  {:<width$}  {}", refusal.code, refusal.when);
            let _ = writeln!(out, "  {:<width$}  → {}", "", refusal.remedy);
        }
    }

    match (command.availability)() {
        crate::spec::Availability::Available => {}
        crate::spec::Availability::Unavailable { reason, remedy, .. } => {
            let _ = writeln!(out, "\nUNAVAILABLE HERE\n  {reason}\n  → {remedy}");
        }
    }

    if let Some(reference) = command.reference {
        let _ = writeln!(out, "\nREFERENCE\n  {reference}");
    }

    out
}

/// The machine form of tier 3. Same facts, same source, no prose layout — so
/// a caller that wants a schema does not have to parse a help screen.
pub fn command_json(command: &Command) -> Value {
    let availability = (command.availability)();
    let mut descriptor = json!({
        "id": command.id,
        "path": command.path,
        "contract": command.contract,
        // Tier 3 only: chapter selection stays out of the cheap indexes.
        "chapter": command.chapter.token(),
        "summary": command.summary,
        "purpose": command.purpose,
        "effect": command.effect.token(),
        "authority": command.authority.token(),
        "execution": command.execution.token(),
        "confirmation_required": command.effect.needs_confirmation(),
        "availability": availability.token(),
        "inputs": command.args.iter().map(arg_json).collect::<Vec<_>>(),
        "output": command.output,
        "examples": command.examples.iter().map(|example| json!({
            "command": example.command,
            "note": example.note,
            // Whether the contract test may execute this verbatim. An example
            // needing a paired desktop, a project, or an operator's own file
            // is documentation; one marked runnable is a promise, and the
            // suite holds it.
            "runnable": example.runnable,
        })).collect::<Vec<_>>(),
        "refusals": command.refusals.iter().map(|refusal| json!({
            "code": refusal.code,
            "when": refusal.when,
            "remedy": refusal.remedy,
        })).collect::<Vec<_>>(),
    });

    if let crate::spec::Availability::Unavailable {
        code,
        reason,
        remedy,
    } = &availability
    {
        descriptor["unavailable"] = json!({ "code": code, "reason": reason, "remedy": remedy });
    }
    if let Some(reference) = command.reference {
        descriptor["reference"] = json!(reference);
    }
    descriptor
}

fn arg_json(arg: &Arg) -> Value {
    let mut value = json!({
        "name": arg.name,
        "kind": match arg.kind {
            ArgKind::Value => "value",
            ArgKind::Switch => "switch",
            ArgKind::Repeated => "repeated",
            ArgKind::Positional => "positional",
        },
        "required": arg.required,
        "summary": arg.summary,
    });
    if arg.kind != ArgKind::Switch {
        value["value"] = json!(arg.value);
    }
    if let Some(default) = arg.default {
        value["default"] = json!(default);
    }
    if !arg.choices.is_empty() {
        value["choices"] = json!(arg.choices);
    }
    value
}

fn flag_form(arg: &Arg) -> String {
    match arg.kind {
        ArgKind::Switch => format!("--{}", arg.name),
        ArgKind::Positional => arg.value.to_string(),
        _ => format!("--{} {}", arg.name, arg.value),
    }
}

fn usage_tail(args: &[Arg]) -> String {
    let mut out = String::new();
    // Operands first, in declaration order: that is the order they must be
    // typed, so it is the order they are shown.
    let ordered = args
        .iter()
        .filter(|arg| arg.kind == ArgKind::Positional)
        .chain(args.iter().filter(|arg| arg.kind != ArgKind::Positional));
    for arg in ordered {
        let form = flag_form(arg);
        if arg.required {
            let _ = write!(out, " {form}");
        } else {
            let _ = write!(out, " [{form}]");
        }
    }
    out
}

/// Wrap at `width` columns with a fixed indent. Deterministic, so golden
/// tests compare text rather than terminal state.
fn wrap(text: &str, width: usize, indent: &str) -> String {
    let mut out = String::new();
    let mut line = String::from(indent);
    for word in text.split_whitespace() {
        if line.len() > indent.len() && line.len() + 1 + word.len() > width {
            out.push_str(&line);
            out.push('\n');
            line = String::from(indent);
        }
        if line.len() > indent.len() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if line.len() > indent.len() {
        out.push_str(&line);
    }
    out
}
