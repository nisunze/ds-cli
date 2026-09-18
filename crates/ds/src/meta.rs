//! The commands that describe the CLI itself.
//!
//! `ds capabilities` is the machine face of the same tiered discovery `--help`
//! gives a person, and it is deliberately *not* one catalog. A caller asking
//! about Solar must not receive the dsgrid, PLS and reporting
//! contracts as the price of the question. So the selector decides the tier:
//!
//!   ds capabilities                  the domain index — a few hundred bytes
//!   ds capabilities dsgrid          one domain's command index
//!   ds capabilities dsgrid.inspect  one complete descriptor
//!   ds capabilities --search "…"     ids and one-liners, nothing more
//!   ds capabilities --requires window  what still cannot run on a Server
//!
//! Search returns identifiers and summaries only. The agent then asks for the
//! one descriptor it chose. That two-step is the whole point: the expensive
//! thing is fetched once, deliberately, after the cheap thing narrowed it.

use ds_cli_contract::help;
use ds_cli_contract::spec::{
    Arg, Authority, Availability, Chapter, Command, Effect, Example, Execution, Refusal, Requires,
};
use ds_cli_contract::{Context, Failure, Inputs};
use serde_json::{Value, json};

use crate::build;
use crate::registry::{self, Entry};

pub static ENTRIES: &[Entry] = &[
    Entry {
        command: &CAPABILITIES,
        handler: capabilities,
        render: render_capabilities,
    },
    Entry {
        command: &DOCTOR,
        handler: doctor,
        render: render_doctor,
    },
    Entry {
        command: &VERSION,
        handler: version,
        render: render_version,
    },
];

fn always() -> Availability {
    Availability::Available
}

// ---------------------------------------------------------------------------
// capabilities
// ---------------------------------------------------------------------------

pub static CAPABILITIES: Command = Command {
    id: "capabilities",
    path: &["capabilities"],
    contract: 1,
    summary: "Discover commands as JSON, one tier at a time.",
    purpose: "\
The machine face of help. With no selector it lists domains. With a domain it \
lists that domain's commands. With a command id it returns that one command's \
complete descriptor. Use --search to find a command by words, then ask for the \
descriptor of the one you chose — search returns ids and summaries only, so \
finding a command costs almost nothing. Use --requires to ask the other \
question — where a command can run at all — across every domain, or inside \
one of them.",
    chapter: Chapter::Catalog,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[
        Arg::positional(
            "selector",
            "[<domain>|<command-id>]",
            "A domain, or a dotted command id.",
        ),
        Arg::value(
            "search",
            "<text>",
            "Match commands by words; returns ids and summaries.",
        ),
        Arg::value(
            "requires",
            "<server|window>",
            "Keep only commands that can run there.",
        )
        .choices(Requires::TOKENS),
        Arg::value("limit", "<n>", "Cap search results.").default("10"),
    ],
    output: "\
A `tier` field naming what came back — `domains`, `commands`, `command`, \
`search` or `requires` — and the matching payload. Descriptors carry effect, \
authority, requires, availability, inputs, refusals and examples. The \
`requires` tier adds the total and the per-domain counts.",
    examples: &[
        Example {
            command: "ds capabilities --output json",
            note: "The domain index. Start here.",
            runnable: true,
        },
        Example {
            command: "ds capabilities dsgrid --output json",
            note: "One domain's commands.",
            runnable: true,
        },
        Example {
            command: "ds capabilities dsgrid.inspect --output json",
            note: "One complete contract.",
            runnable: true,
        },
        Example {
            command: "ds capabilities --search \"dsgrid model\" --output json",
            note: "Ids and one-liners; fetch the descriptor you want next.",
            runnable: true,
        },
        Example {
            command: "ds capabilities --requires window --output json",
            note: "What still needs the application, counted by domain.",
            runnable: true,
        },
        Example {
            command: "ds capabilities survey --requires window --output json",
            note: "`matched` 0 proves a domain is server-first.",
            runnable: true,
        },
    ],
    refusals: &[
        Refusal {
            code: "unknown_selector",
            when: "the selector is neither a domain nor a command id",
            remedy: "run `ds capabilities` for the domain index",
        },
        Refusal {
            code: "conflicting_selector",
            when: "--requires is combined with --search or a command id",
            remedy: "ask `ds capabilities --requires <server|window> [<domain>]` on its own",
        },
    ],
    reference: None,
    requires: Requires::Server,
    availability: always,
};

fn capabilities(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let schema_only = std::env::var_os("DS_CLI_SCHEMA_ONLY").is_some_and(|value| !value.is_empty());

    if let Some(token) = inputs.value("requires") {
        return requires(
            token,
            inputs.value("selector"),
            inputs.value("search"),
            inputs.value("limit").unwrap_or("10"),
        );
    }

    if let Some(query) = inputs.value("search") {
        return search(query, inputs.value("limit").unwrap_or("10"));
    }

    let Some(selector) = inputs.value("selector") else {
        // Tier 1. Domains only — this is the response an agent pays for on
        // every cold start, so it stays a few hundred bytes forever.
        return Ok(json!({
            "tier": "domains",
            "domains": registry::domains()
                .iter()
                .map(|registered| json!({
                    "id": registered.domain.id,
                    "summary": registered.domain.summary,
                    "commands": registered.entries.len(),
                }))
                .collect::<Vec<_>>(),
            "next": "ds capabilities <domain>",
        }));
    };

    if let Some(registered) = registry::find_domain(selector) {
        // Tier 2. One domain's index: ids and one-liners, plus the one fact
        // that changes what a caller can do right now.
        return Ok(json!({
            "tier": "commands",
            "domain": registered.domain.id,
            "commands": registered.entries
                .iter()
                .map(|entry| json!({
                    "id": entry.command.id,
                    "summary": entry.command.summary,
                    "effect": entry.command.effect.token(),
                    "authority": entry.command.authority.token(),
                    "availability": if schema_only {
                        "unchecked"
                    } else {
                        (entry.command.availability)().token()
                    },
                }))
                .collect::<Vec<_>>(),
            "next": "ds capabilities <command-id>",
        }));
    }

    if let Some(entry) = registry::find_by_id(selector) {
        // Tier 3. Exactly one contract.
        return Ok(json!({
            "tier": "command",
            "command": if schema_only {
                help::command_json_unchecked(entry.command)
            } else {
                help::command_json(entry.command)
            },
        }));
    }

    let known: Vec<&str> = registry::domains()
        .iter()
        .map(|registered| registered.domain.id)
        .collect();
    let mut failure = Failure::invalid(
        "unknown_selector",
        format!("`{selector}` is neither a domain nor a command id"),
    );
    let ids: Vec<&str> = registry::all_commands()
        .iter()
        .map(|command| command.id)
        .collect();
    if let Some(suggestion) =
        ds_cli_contract::args::nearest(selector, known.iter().copied().chain(ids.iter().copied()))
    {
        failure = failure.remedy(format!("did you mean `{suggestion}`?"));
    } else {
        failure = failure.remedy("run `ds capabilities` for the domain index");
    }
    Err(failure
        .next("ds capabilities")
        .detail(json!({ "domains": known })))
}

/// Where commands can run, asked as a question instead of read one
/// descriptor at a time.
///
/// This is the aggregate the per-command fact exists for: "what still needs
/// the window?" is a single call, and "is this domain server-first?" is the
/// same call with the domain named. It answers from the declared
/// [`Requires`] fact, never from a live probe, so the answer does not change
/// when the application happens to be running.
///
/// Bounded like every other projection here: the total and the per-domain
/// counts always come back — they are what the question is usually for — and
/// the ids themselves are a page capped the same way search is, with `more`
/// naming what was left out.
fn requires(
    token: &str,
    selector: Option<&str>,
    search_query: Option<&str>,
    limit: &str,
) -> Result<Value, Failure> {
    // Two filters over the same set would leave the caller guessing which one
    // shaped the answer, and a command id already carries `requires` in its
    // own descriptor.
    let conflict = |what: &str, instead: String| {
        Err(Failure::invalid(
            "conflicting_selector",
            format!("--requires cannot be combined with {what}"),
        )
        .remedy(instead)
        .next("ds capabilities --requires window"))
    };
    if search_query.is_some() {
        return conflict(
            "--search",
            "ask one question at a time: `ds capabilities --requires window`, \
             or `ds capabilities --search \"…\"`"
                .to_owned(),
        );
    }

    let wanted = Requires::from_token(token).ok_or_else(|| {
        Failure::invalid(
            "conflicting_selector",
            format!("`{token}` is not a place a command can run"),
        )
        .remedy(format!("use one of: {}", Requires::TOKENS.join(", ")))
    })?;

    let mut domains: Vec<&'static registry::Registered> = Vec::new();
    match selector {
        None => domains.extend(registry::domains()),
        Some(name) => match registry::find_domain(name) {
            Some(registered) => domains.push(registered),
            None if registry::find_by_id(name).is_some() => {
                return conflict(
                    "a command id",
                    format!("read `requires` in `ds capabilities {name}`"),
                );
            }
            None => {
                return Err(Failure::invalid(
                    "unknown_selector",
                    format!("`{name}` is not a domain"),
                )
                .remedy("run `ds capabilities` for the domain index")
                .next("ds capabilities"));
            }
        },
    }

    let limit: usize = limit.parse().unwrap_or(10).clamp(1, 50);
    let mut counts: Vec<Value> = Vec::new();
    let mut matched: Vec<(&'static str, &'static Command)> = Vec::new();
    for registered in domains {
        let hits: Vec<&'static Command> = registered
            .entries
            .iter()
            .map(|entry| entry.command)
            .filter(|command| command.requires == wanted)
            .collect();
        if hits.is_empty() {
            continue;
        }
        counts.push(json!({
            "id": registered.domain.id,
            "commands": hits.len(),
        }));
        matched.extend(
            hits.into_iter()
                .map(|command| (registered.domain.id, command)),
        );
    }

    let total = matched.len();
    matched.sort_by_key(|(_, command)| command.id);
    matched.truncate(limit);

    let mut result = json!({
        "tier": "requires",
        "requires": wanted.token(),
        "matched": total,
        "domains": counts,
        "results": matched
            .iter()
            .map(|(domain, command)| json!({
                "id": command.id,
                "domain": domain,
                "summary": command.summary,
            }))
            .collect::<Vec<_>>(),
        "next": "ds capabilities <command-id>",
    });
    if let Some(name) = selector {
        result["domain"] = json!(name);
    }
    if total > matched.len() {
        result["more"] = json!({
            "reason": "limit_reached",
            "shown": matched.len(),
            "matched": total,
            "next": "read `domains` for the whole count, or raise --limit (max 50)",
        });
    }
    Ok(result)
}

/// Word-overlap search over ids, summaries and purposes. Deliberately simple
/// and deliberately shallow: it returns a shortlist to choose from, not an
/// answer, so precision matters far less than never hiding a real match.
fn search(query: &str, limit: &str) -> Result<Value, Failure> {
    let limit: usize = limit.parse().unwrap_or(10).clamp(1, 50);
    let terms: Vec<String> = query
        .split_whitespace()
        .map(|term| term.to_lowercase())
        .filter(|term| term.len() > 1)
        .collect();

    if terms.is_empty() {
        return Err(
            Failure::invalid("empty_search", "--search needs at least one word")
                .remedy("try `ds capabilities --search \"model inspect\"`"),
        );
    }

    let mut scored: Vec<(usize, &'static Command)> = registry::all_commands()
        .into_iter()
        .filter_map(|command| {
            let haystack =
                format!("{} {} {}", command.id, command.summary, command.purpose).to_lowercase();
            let score = terms
                .iter()
                .filter(|term| haystack.contains(term.as_str()))
                .count();
            (score > 0).then_some((score, command))
        })
        .collect();

    // Highest score first, then by id so equal matches are stably ordered.
    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.id.cmp(right.1.id)));
    let total = scored.len();
    scored.truncate(limit);

    Ok(json!({
        "tier": "search",
        "query": query,
        "matched": total,
        "results": scored
            .iter()
            .map(|(score, command)| json!({
                "id": command.id,
                "summary": command.summary,
                "terms_matched": score,
            }))
            .collect::<Vec<_>>(),
        "next": "ds capabilities <command-id>",
    }))
}

fn render_capabilities(data: &Value) -> String {
    let mut out = String::new();
    match data["tier"].as_str().unwrap_or("") {
        "domains" => {
            for domain in data["domains"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{:<10}  {:>2}  {}\n",
                    domain["id"].as_str().unwrap_or(""),
                    domain["commands"],
                    domain["summary"].as_str().unwrap_or(""),
                ));
            }
        }
        "requires" => {
            for domain in data["domains"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{:<10}  {:>3}\n",
                    domain["id"].as_str().unwrap_or(""),
                    domain["commands"],
                ));
            }
            out.push_str(&format!(
                "\n{} command(s) require {}\n",
                data["matched"],
                data["requires"].as_str().unwrap_or(""),
            ));
            for command in data["results"].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{:<26}  {}\n",
                    command["id"].as_str().unwrap_or(""),
                    command["summary"].as_str().unwrap_or(""),
                ));
            }
            if let Some(more) = data["more"]["next"].as_str() {
                out.push_str(&format!("({more})\n"));
            }
        }
        "commands" | "search" => {
            let key = if data["tier"] == "search" {
                "results"
            } else {
                "commands"
            };
            for command in data[key].as_array().into_iter().flatten() {
                out.push_str(&format!(
                    "{:<22}  {}\n",
                    command["id"].as_str().unwrap_or(""),
                    command["summary"].as_str().unwrap_or(""),
                ));
            }
        }
        _ => {
            let command = &data["command"];
            out.push_str(&format!(
                "{}  (contract v{})\n{}\n  effect {}  authority {}  {}\n",
                command["id"].as_str().unwrap_or(""),
                command["contract"],
                command["summary"].as_str().unwrap_or(""),
                command["effect"].as_str().unwrap_or(""),
                command["authority"].as_str().unwrap_or(""),
                command["availability"].as_str().unwrap_or(""),
            ));
        }
    }
    if let Some(next) = data["next"].as_str() {
        out.push_str(&format!("\nnext: {next}\n"));
    }
    out
}

// ---------------------------------------------------------------------------
// doctor
// ---------------------------------------------------------------------------

pub static DOCTOR: Command = Command {
    id: "doctor",
    path: &["doctor"],
    contract: 2,
    summary: "Report what works on this machine, and why not.",
    purpose: "\
Resolves every registered command's availability and reports the ones that \
cannot run here, each with the concrete thing that would fix it. Availability \
checks are domain-local and cheap. Doctor also verifies the packaged agent \
skill bundle and reports whether each supported user skill directory has the \
matching install, and whether `ds` is reachable from this shell and from a \
new one. Its build object includes the compile-time ds-network source pin or \
the explicit development-unpinned state; it does not introspect linked Rust \
code at runtime. It starts no engine and probes no network. Use it first on \
an unfamiliar machine.",
    chapter: Chapter::Catalog,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[Arg::switch(
        "all",
        "List available commands too, not just blocked ones.",
    )],
    output: "\
Counts of available and unavailable commands, one entry per unavailable \
command with its reason and remedy, build identity including linked ds-network \
compile-time provenance, and agent skill bundle and \
install status, and shell reach. With --all, every command.",
    examples: &[
        Example {
            command: "ds doctor",
            note: "",
            runnable: true,
        },
        Example {
            command: "ds doctor --output json",
            note: "Branch on .data.unavailable being empty.",
            runnable: true,
        },
    ],
    refusals: &[],
    reference: None,
    requires: Requires::Server,
    availability: always,
};

fn doctor(inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    let all = inputs.switch("all");
    let mut available = 0usize;
    let mut blocked: Vec<Value> = Vec::new();
    let mut listed: Vec<Value> = Vec::new();

    for command in registry::all_commands() {
        match (command.availability)() {
            Availability::Available => {
                available += 1;
                if all {
                    listed.push(json!({ "id": command.id, "availability": "available" }));
                }
            }
            Availability::Unavailable {
                code,
                reason,
                remedy,
            } => {
                let entry = json!({
                    "id": command.id,
                    "availability": "unavailable",
                    "code": code,
                    "reason": reason,
                    "remedy": remedy,
                });
                blocked.push(entry.clone());
                if all {
                    listed.push(entry);
                }
            }
        }
    }

    let mut report = json!({
        "available": available,
        "unavailable": blocked,
        "build": build::identity(),
        "shell": ds_cli_shell::report(),
        "skills": ds_cli_skills::doctor_report(build::SOURCE_SHA),
    });
    if all {
        report["commands"] = Value::Array(listed);
    }
    Ok(report)
}

fn render_doctor(data: &Value) -> String {
    let blocked = data["unavailable"].as_array().map_or(0, Vec::len);
    let mut out = format!(
        "{} command(s) available, {blocked} blocked\n",
        data["available"]
    );
    for entry in data["unavailable"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "\n{}\n  {}\n  → {}\n",
            entry["id"].as_str().unwrap_or(""),
            entry["reason"].as_str().unwrap_or(""),
            entry["remedy"].as_str().unwrap_or(""),
        ));
    }
    let skills = &data["skills"];
    out.push_str(&format!(
        "\nagent skills: {}\n",
        skills["status"].as_str().unwrap_or("unknown")
    ));
    if let Some(path) = skills["bundle_path"].as_str() {
        out.push_str(&format!("  bundle: {path}\n"));
    }
    for agent in skills["agents"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "  {:<7} {}\n",
            agent["agent"].as_str().unwrap_or("agent"),
            agent["status"].as_str().unwrap_or("unknown"),
        ));
    }
    if let Some(reason) = skills["reason"].as_str() {
        out.push_str(&format!("  {reason}\n"));
    }
    if let Some(remedy) = skills["remedy"].as_str() {
        out.push_str(&format!("  remedy: {remedy}\n"));
    }
    let shell = &data["shell"];
    out.push_str(&format!(
        "\nshell: {}",
        shell["status"].as_str().unwrap_or("unknown")
    ));
    if let Some(executable) = shell["executable"].as_str() {
        out.push_str(&format!("  {executable}"));
    }
    out.push('\n');
    if let Some(reason) = shell["reason"].as_str() {
        out.push_str(&format!("  {reason}\n"));
    }
    if let Some(remedy) = shell["remedy"].as_str() {
        out.push_str(&format!("  remedy: {remedy}\n"));
    }
    out
}

// ---------------------------------------------------------------------------
// version
// ---------------------------------------------------------------------------

pub static VERSION: Command = Command {
    id: "version",
    path: &["version"],
    contract: 1,
    summary: "Report verifiable build identity.",
    purpose: "\
Reports the exact source this binary was built from, its target triple, \
profile and whether the tree was dirty, the pinned native-client core, the \
compile-time clean ds-network release pin, and the contract versions it \
speaks. The ds-network field is build provenance, not a claim of runtime \
introspection into linked Rust code. Packaging verifies a staged executable by running this, so the answer \
has to come from the build rather than from a string someone maintains.",
    chapter: Chapter::Catalog,
    effect: Effect::Discovery,
    authority: Authority::None,
    execution: Execution::Sync,
    args: &[],
    output: "Product name, release version, CLI, native-client-core, and linked ds-network source provenance, dirty flag, target, profile, and envelope version.",
    examples: &[Example {
        command: "ds version --output json",
        note: "Packaging asserts .data.source_sha against its pin.",
        runnable: true,
    }],
    refusals: &[],
    reference: None,
    requires: Requires::Server,
    availability: always,
};

fn version(_inputs: &Inputs, _context: &Context) -> Result<Value, Failure> {
    Ok(build::identity())
}

fn render_version(data: &Value) -> String {
    format!(
        "{} {}  {}{}\n{} · {}",
        data["product"].as_str().unwrap_or("ds"),
        data["version"].as_str().unwrap_or(""),
        data["source_sha"].as_str().unwrap_or("unknown"),
        if data["dirty"].as_bool().unwrap_or(false) {
            " (dirty)"
        } else {
            ""
        },
        data["target"].as_str().unwrap_or(""),
        data["profile"].as_str().unwrap_or(""),
    )
}
