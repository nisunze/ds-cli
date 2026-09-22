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
    search: &[],
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
                    // The same reading tier 3 publishes: a window-bound
                    // command says so rather than certifying itself
                    // available on a machine with no window paired.
                    "availability": if schema_only {
                        "unchecked"
                    } else {
                        ds_cli_contract::help::reported_availability(
                            entry.command,
                            Some(&(entry.command.availability)()),
                        )
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

/// Where a term was found. A hit in a command's own name is evidence of what
/// the command *is*; a hit deep in a paragraph is evidence it was mentioned.
/// Ranking by that difference is the whole reason this is not `contains`.
#[derive(Clone, Copy)]
enum Field {
    /// The dotted id and invocation path — what the command is called.
    Name,
    /// The declared finding aid: the outside words for this command.
    Term,
    /// The one-line summary.
    Summary,
    /// The paragraph. A mention, not a name.
    Purpose,
}

impl Field {
    /// The weight of one whole-word hit in this field.
    const fn weight(self) -> u32 {
        match self {
            Self::Name => 100,
            Self::Term => 90,
            Self::Summary => 40,
            Self::Purpose => 10,
        }
    }

    /// The weight of a hit that is only a prefix of a longer word — `buffer`
    /// inside `buffered`. Real, but never worth more than an exact name hit,
    /// so a stem can surface a command and still not outrank the command
    /// actually called that.
    const fn stem_weight(self) -> u32 {
        self.weight() / 4
    }
}

/// Split text into lowercase words. Anything that is not a letter or a digit
/// separates: `map.layer.add`, `map layer add` and `map-layer-add` are the
/// same three words, which is what lets an id be searched like prose.
fn words(text: &str) -> Vec<String> {
    text.split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(str::to_lowercase)
        .collect()
}

/// Score one term against one field's words.
///
/// A whole word scores full. A word that shares a stem with the term scores a
/// quarter, which is how `buffer` still finds `buffered` without `line`
/// finding `Link` — `Link` neither equals `line` nor shares a stem with it.
/// That single distinction is what stopped a search for `line` returning
/// `assets.attach` ("**Lin**k an asset…") ahead of every real line command.
///
/// The stem is read in BOTH directions, because a stranger types the plural
/// as readily as the singular: `buffers` has to find `buffer`, or the one
/// query most likely to be typed at this family returns a cache command and
/// nothing else. Four characters is the floor in either direction, so a
/// fragment like `lin` still matches nothing.
///
/// The two directions are not symmetrical, though. A term that *extends* a
/// word by more than an inflection is a different word — `overlay` is not a
/// kind of `over`, and `shapefile` is not a kind of `shape` — so that
/// direction is capped at two extra characters, which is a plural and little
/// else. A term the word extends keeps the looser rule: `print` genuinely is
/// what `printing` is about.
///
/// One more rule, and it is about us rather than about English: our own
/// product prefix must not hide a stranger's word. `dsgrid` is `ds` + `grid`,
/// and a word-boundary matcher made eleven `dsgrid` commands unreachable by
/// the one word an outsider would type for them. Only our own two letters
/// count, so this cannot become the `Link`/`line` coincidence again —
/// `export` still does not answer to `port`. It scores as the whole word it
/// is, not as a stem: in our own vocabulary `dsgrid` IS `grid`.
fn score_term(term: &str, haystack: &[String], field: Field) -> u32 {
    let mut best = 0;
    for word in haystack {
        let inflection = word.len() >= 4
            && term.len() > word.len()
            && term.len() - word.len() <= 2
            && term.starts_with(word.as_str());
        let ours = word.len() > 2 && word.strip_prefix("ds") == Some(term);
        let shares_stem = (term.len() >= 4 && word.starts_with(term)) || inflection;
        let hit = if word == term || ours {
            field.weight()
        } else if shares_stem {
            field.stem_weight()
        } else {
            0
        };
        best = best.max(hit);
    }
    best
}

/// How well one command answers a query, and how many of the query's terms it
/// accounted for.
fn score_command(terms: &[String], command: &'static Command) -> (u32, usize) {
    let name = words(&format!("{} {}", command.id, command.path.join(" ")));
    let declared = words(&command.search.join(" "));
    let summary = words(command.summary);
    let purpose = words(command.purpose);

    let mut total = 0;
    let mut matched = 0;
    for term in terms {
        let best = score_term(term, &name, Field::Name)
            .max(score_term(term, &declared, Field::Term))
            .max(score_term(term, &summary, Field::Summary))
            .max(score_term(term, &purpose, Field::Purpose));
        if best > 0 {
            matched += 1;
            total += best;
        }
    }
    // A command that answers every word of the query is a different kind of
    // answer from one that answers half of it, however loudly.
    if matched == terms.len() {
        total += 50 * matched as u32;
    }
    (total, matched)
}

/// Ranked search over ids, declared terms, summaries and purposes.
///
/// It returns a shortlist to choose from, not an answer. What it owes the
/// caller is that the shortlist's FIRST entry is the one they meant: an agent
/// that has to read ten rows to find a buffer command has already paid more
/// context than the search saved.
fn search(query: &str, limit: &str) -> Result<Value, Failure> {
    let limit: usize = limit.parse().unwrap_or(10).clamp(1, 50);
    let terms: Vec<String> = words(query)
        .into_iter()
        .filter(|term| term.len() > 1)
        .collect();

    if terms.is_empty() {
        return Err(
            Failure::invalid("empty_search", "--search needs at least one word")
                .remedy("try `ds capabilities --search \"model inspect\"`"),
        );
    }

    let mut scored: Vec<(u32, usize, &'static Command)> = registry::all_commands()
        .into_iter()
        .filter_map(|command| {
            let (score, matched) = score_command(&terms, command);
            (score > 0).then_some((score, matched, command))
        })
        .collect();

    // Best score first, then by id so equal matches are stably ordered.
    scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.2.id.cmp(right.2.id)));
    let total = scored.len();
    scored.truncate(limit);

    let mut result = json!({
        "tier": "search",
        "query": query,
        "matched": total,
        "results": scored
            .iter()
            .map(|(_, matched, command)| json!({
                "id": command.id,
                "summary": command.summary,
                "terms_matched": matched,
            }))
            .collect::<Vec<_>>(),
        "next": "ds capabilities <command-id>",
    });

    // Nothing found is an answer too, and the worst possible version of it is
    // an empty list with no next step: that is where an outside agent gives
    // up on `ds` and installs its own toolchain instead.
    if total == 0 {
        // "Nothing matched these words" is the fact. "The capability is
        // missing" is a claim this search cannot make — it reads ids, declared
        // terms, summaries and purposes, so an unindexed word is a silence
        // about the vocabulary, not about the product. Saying otherwise is how
        // a caller files a gap report for a capability that ships.
        result["next"] = json!(
            "nothing matched these words; `ds capabilities` lists every domain \
             and `ds capabilities <domain>` its commands — try other words \
             first, and `ds feedback submit` if the capability really is absent"
        );
    } else if total > scored.len() {
        // Terse on purpose. Every search pays for this line, and what the
        // caller needs from it is two facts: that the list was cut, and the
        // flag that uncuts it.
        result["more"] = json!(format!(
            "{} not shown; --limit up to 50",
            total - scored.len()
        ));
    }
    Ok(result)
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
    // Truncation a reader cannot see is worse than truncation, so it is said
    // in text as well as in JSON. A caller who does not know the list was cut
    // reads ten rows believing they have read the surface.
    if let Some(more) = data["more"].as_str() {
        out.push_str(&format!("\nmore: {more}\n"));
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
    search: &[],
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
    search: &[],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn top_hit(query: &str) -> Option<&'static str> {
        let mut scored: Vec<(u32, &'static Command)> = registry::all_commands()
            .into_iter()
            .filter_map(|command| {
                let (score, _) = score_command(&words(query), command);
                (score > 0).then_some((score, command))
            })
            .collect();
        scored.sort_by(|left, right| right.0.cmp(&left.0).then(left.1.id.cmp(right.1.id)));
        scored.first().map(|(_, command)| command.id)
    }

    /// The failure this matcher was rewritten for.
    ///
    /// Searching `line` used to return 40 results led by `assets.attach`,
    /// whose summary begins "**Lin**k an asset…". An outside agent reading
    /// from the top found link management where it asked for lines, and gave
    /// up on `ds`. A word is a word: `Link` neither equals `line` nor starts
    /// with it, so it scores nothing at all now.
    #[test]
    fn a_term_does_not_match_inside_an_unrelated_word() {
        let haystack = words("Link an asset to a task or a DS object.");
        assert_eq!(score_term("line", &haystack, Field::Summary), 0);
        assert_eq!(
            score_term("link", &haystack, Field::Summary),
            Field::Summary.weight()
        );
    }

    /// A stem still finds its word, at a quarter weight, so a command named
    /// for the term always outranks one that merely inflects it.
    #[test]
    fn a_stem_is_found_but_never_outranks_the_real_name() {
        let inflected = words("buffered corridor");
        assert_eq!(
            score_term("buffer", &inflected, Field::Summary),
            Field::Summary.stem_weight()
        );
        assert!(Field::Summary.stem_weight() < Field::Name.weight());
        // Three letters is noise, not a stem: `map` must not match `mapping
        // reports` everywhere in the surface.
        assert_eq!(score_term("lin", &words("linear"), Field::Summary), 0);

        // The plural a stranger actually types has to reach the singular we
        // chose. `buffers` found only a cache command before this.
        assert_eq!(
            score_term("buffers", &words("buffer a geometry"), Field::Summary),
            Field::Summary.stem_weight()
        );
        assert_eq!(score_term("king", &words("kin"), Field::Summary), 0);

        // …but a longer word an outsider's term merely begins with is a
        // different word: `overlay` is not a kind of `over`.
        assert_eq!(
            score_term("overlay", &words("over the wire"), Field::Summary),
            0
        );
        assert_eq!(
            score_term("shapefile", &words("shape and size"), Field::Summary),
            0
        );

        // Our own two letters must not hide a word from a stranger: `dsgrid`
        // is `ds` + `grid`, and eleven commands were unreachable by the one
        // word an outsider types for that family.
        // It scores as the whole word it is: in our vocabulary `dsgrid` IS
        // `grid`, so a command named for it outranks one that merely says the
        // word in a sentence.
        assert_eq!(
            score_term("grid", &words("dsgrid.model.list"), Field::Name),
            Field::Name.weight()
        );
        // …and only our own two letters, so this is not `Link`/`line` again.
        assert_eq!(
            score_term("port", &words("export a file"), Field::Summary),
            0
        );
    }

    /// An id hit is evidence of what a command IS. A purpose hit is evidence
    /// it was mentioned. Ranking has to tell those apart or the shortlist is
    /// alphabetical noise, which is what it was.
    #[test]
    fn where_a_term_was_found_decides_the_order() {
        assert!(Field::Name.weight() > Field::Summary.weight());
        assert!(Field::Summary.weight() > Field::Purpose.weight());
        assert!(Field::Term.weight() > Field::Summary.weight());
    }

    /// The measured queries an engineer's agent actually typed, each pinned to
    /// the command it should have found. Before this slice every one of these
    /// returned nothing, or returned the wrong command first.
    #[test]
    fn an_outsiders_words_land_on_the_right_command_first() {
        for (query, expected) in [
            ("buffer", "data.vector.buffer"),
            ("geoprocessing", "data.vector.buffer"),
            ("intersect", "data.vector.intersect"),
            ("overlay", "data.vector.intersect"),
            ("line", "map.line-difference"),
            ("chainage", "data.vector.sample"),
            ("crs", "data.convert"),
            ("shapefile", "data.convert"),
            ("length", "data.vector.measure"),
            // Holding the country's data is something an agent should reach
            // for with its own words, on the way to a report, without a human
            // naming the command. Each of these led somewhere else.
            ("cache rwanda data", "desktop.data.rwanda.install"),
            ("download datasets", "desktop.data.rwanda.install"),
            ("install reference data", "desktop.data.rwanda.install"),
            ("ground data", "desktop.data.rwanda.install"),
        ] {
            assert_eq!(
                top_hit(query),
                Some(expected),
                "`ds capabilities --search {query}` must lead with `{expected}`"
            );
        }
    }

    /// A declared term is a finding aid, not a second description. Nothing
    /// reads these but the matcher, and the one-description rule holds only
    /// while they stay words.
    #[test]
    fn declared_search_terms_are_words_not_prose() {
        for command in registry::all_commands() {
            let mut seen: Vec<&str> = Vec::new();
            for term in command.search {
                assert!(
                    term.len() >= 2 && term.len() <= 24,
                    "`{}` declares `{term}`; a search term is a word, not a sentence",
                    command.id
                );
                assert!(
                    *term == term.to_lowercase(),
                    "`{}` declares `{term}`; search terms are lowercase",
                    command.id
                );
                assert!(
                    term.split_whitespace().count() <= 2,
                    "`{}` declares `{term}`; at most two words per term",
                    command.id
                );
                assert!(
                    !seen.contains(term),
                    "`{}` declares `{term}` twice",
                    command.id
                );
                seen.push(term);
            }
        }
    }

    /// Terms that only repeat what the id or summary already says cost bytes
    /// and buy nothing. The field is for the words we did NOT choose.
    #[test]
    fn a_declared_term_says_something_the_command_does_not_already_say() {
        for command in registry::all_commands() {
            let own = words(&format!("{} {}", command.id, command.summary));
            for term in command.search {
                assert!(
                    !own.contains(&term.to_string()),
                    "`{}` declares `{term}`, which its id or summary already says",
                    command.id
                );
            }
        }
    }
}
