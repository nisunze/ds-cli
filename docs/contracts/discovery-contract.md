# Discovery contract — `ds-cli-discovery/v1`

**Status:** active contract. A change that violates it is a rejectable change.

`ds` is one executable holding the whole Data Solutions stack. That is only
workable — for a person and for a coding agent — if discovery is *tiered*, so
nobody pays the context cost of a domain they are not using.

This document states the rules. `crates/ds/tests/context_budget.rs` enforces
them in bytes.

## The four tiers

| Tier | Command | Contains | Never contains |
|---|---|---|---|
| 1 Root | `ds --help` | identity, one line per **domain**, how to drill down, global flags | any command name |
| 2 Domain | `ds <domain> --help` | one line per command in that domain | any command's prose, any other domain |
| 3 Command | `ds <domain> <cmd> --help` | one complete contract | any other command |
| 4 Reference | a path named in tier 3 | long-form explanation | anything loaded automatically |

Each tier is reached by an explicit call. No tier prints the tier below it.

The machine-readable companion uses the same declarations:

```bash
ds capabilities                    # tier 1 — domain index
ds capabilities dsgrid            # tier 2 — one domain's commands
ds capabilities dsgrid.inspect    # tier 3 — one full descriptor
ds capabilities --search "text"    # ids and one-liners, nothing more
```

Search deliberately returns identifiers and summaries only. The caller then
asks for the one descriptor it chose. Two cheap calls beat one expensive one.

**Compact discovery never replaces help.** Root, domain and command help are
permanent first-class interfaces for people and remain complete at their own
tier. `capabilities` exists so a machine can select and parse a bounded slice;
it is a projection of the same declaration, not a successor to `--help`.

## The rule that makes one binary viable

**Root help is proportional to the number of domains, never to the number of
commands.**

Adding a command to an existing domain must not change root help by one byte.
Adding a domain must cost exactly its one-line summary. This is asserted, with
an explicit per-domain allowance, by `root_help_scales_with_domains_not_commands`.

If that test ever fails, someone has started listing commands at the root —
which is the specific failure this CLI exists to avoid.

## Byte budgets

Enforced by `crates/ds/tests/context_budget.rs`. They sit close to today's
sizes on purpose: they are meant to fire when text lands in the wrong tier,
not to leave a year of room for drift.

| Surface | Budget | Today |
|---|---|---|
| `ds --help` | 800 + 80/domain | 949 (2 domains) |
| `ds <domain> --help` | 900 | 263–270 |
| `ds <cmd> --help` | 3 200 | 1 344–2 308 |
| `ds capabilities` (JSON) | 900 | 336 |
| `ds capabilities <domain>` (JSON) | 1 200 | 315 |
| `ds capabilities <id>` (JSON) | 3 500 | 2 498 |
| `ds capabilities --search` (JSON) | 1 200 | 272 |
| default command result (JSON) | 1 500 | 525 |
| error envelope (JSON) | 800 | 277 |

Raising a budget is allowed. Raising one silently is not: change the number in
the test, in the same commit, with the reason.

**Cost of a cold start.** An agent that has never seen `ds` reaches a specific
command's full contract in three calls totalling ~3.1 KB of JSON (336 + 315 +
2 498), or ~3.5 KB of human help.

## Domain discovery must not probe other domains

Resolving one domain's availability may not initialize, spawn, or reach into
another domain's engine. `ds dsgrid --help` performs no desktop lookup;
`ds desktop --help` links no grid engine at runtime.

Structurally: each domain is its own crate, and `availability` is a plain
function on the command that owns it. `ds doctor` is the one command that
resolves every domain's availability, because that is its whole job.

## Availability is answered, not assumed

A command that cannot run here reports `unavailable` with:

- a **code** — the domain's own stable identifier, not a generic token;
- a **reason** — what is missing;
- a **remedy** — the concrete thing that would fix it.

A reason without a remedy is a dead end for an agent and is rejected by
`refusals_are_named_and_actionable`.

**A diagnostic never gates itself.** `ds desktop status` reports whether a
desktop is paired, so it is always available: gating it on a paired desktop
would mean the one call that could explain the situation is the one call that
refuses to. "Not paired" is an answer.

## Bounded by default, more on request

Large results are never inlined by default. A command returns its cheapest
useful projection and names the rest:

```json
"more": {
  "available_projections": ["tables", "members", "library", "extent"],
  "truncated": [{ "field": "tables", "withheld": 33, "limit": 2 }]
}
```

Truncation is always visible. A silently shortened list reads as a complete
one, which is worse than refusing.

Where a projection is expensive, the response says so — `dsgrid inspect`
reports `decoded: true` only when it had to decode the model's tables.

## Help and machine descriptors come from one declaration

Every fact about a command lives once, in its `Command` value
(`crates/ds-cli-contract/src/spec.rs`). Help text, the JSON descriptor,
argument validation and dispatch all read that same value, so they cannot
drift. `command_help_matches_its_descriptor` proves it by comparison.

That shared source prevents divergence; it does not make either presentation
optional. A change that preserves JSON while removing or hollowing out help
violates this contract.

A command that gains a flag without declaring it there cannot receive it.

## Examples are executable

An example marked `runnable` is executed verbatim by
`runnable_examples_run_and_fail_only_as_documented`. It must either succeed or
fail with a code the command **documented** in its refusal list.

That is a weaker promise than "always exits 0", and a truer one: `ds desktop
status` cannot succeed identically on a laptop running the application and on
a CI box without it. What must always hold is that the invocation is valid and
that any failure was foreseen.
