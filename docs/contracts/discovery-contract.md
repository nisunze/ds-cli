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

Enforced by `crates/ds/tests/context_budget.rs`. The important property is that
the ceilings are **derived, not flat**. A command that genuinely declares
twelve inputs must be allowed to describe twelve inputs; what a ceiling is for
is catching text that landed in the wrong tier — an explanation that grew, an
example that became a tutorial, an architecture note that should have been a
cross-link. So each allowance is a small frame plus an amount per declared
thing, and the formula is the contract:

| Surface | Ceiling | Asserted by |
|---|---|---|
| `ds --help` | 800 + 80 per **domain** | `root_help_scales_with_domains_not_commands` |
| `ds <domain> --help` | 260 + 140 per command in that domain | `domain_help_scales_with_its_own_commands` |
| `ds <domain> <cmd> --help` | 1 200 + 180 per declared input + 220 per declared refusal | `command_help_is_bounded` |
| `ds capabilities` (JSON) | 400 + 140 per domain | `discovery_indexes_are_cheap_in_json` |
| `ds capabilities <domain>` (JSON) | 300 + 220 per command in that domain | `discovery_indexes_are_cheap_in_json` |
| `ds capabilities <id>` (JSON) | that command's help ceiling, plus 600 for JSON's own punctuation | `command_descriptors_are_bounded` |

Three surfaces have nothing to scale against, so they carry a flat cap:

| Surface | Cap | Asserted by |
|---|---|---|
| `ds capabilities --search` (JSON) | 1 320 — search is already bounded by its ten-result cap, so this prices the tenth id/summary row | `discovery_indexes_are_cheap_in_json` |
| default command result (JSON) | 1 500 | `default_results_are_bounded` |
| error envelope (JSON) | 800 | `errors_are_short` |

Root help carries a flat cap *in addition to* its derived one, in
`root_help_is_cheap`, so that a raise is a deliberate edit with a written
reason rather than an arithmetic side effect of registering a domain. It is
held at or above the derived ceiling on purpose: if the flat number bound
first, the scaling assertion — the one this entire contract rests on — could
never be the test that fails.

Measured at this commit, for scale and not as a promise: `ds --help` is 2 043
bytes across 16 domains; domain help runs 341 (`sre`) to 2 740 (`map`); command
help runs 749 to 6 798 across 117 commands; `ds capabilities` is 1 721 bytes
and a single descriptor 892 to 6 948; a representative search is 1 299; a
default `dsgrid inspect` result is 524; an error envelope is 291.

Raising a budget is allowed. Raising one silently is not: change the number in
the test, in the same commit, with the reason it moved. The comments in that
file are the record of what each domain cost, and a budget that only ever gets
relaxed is protecting nothing.

**Cost of a cold start.** An agent that has never seen `ds` reaches a specific
command's full contract in three calls — the domain index, one domain's command
list, one descriptor — and pays for one domain rather than all of them. For
`dsgrid.inspect` at this commit that is 1 721 + 813 + 2 984 = 5 518 bytes of
JSON, or 2 043 + 492 + 2 737 = 5 272 bytes of human help.

Only the first of the three grows with the rest of the stack, and it grows by
one summary line per domain. That is the claim worth making: on a `ds` with
twice as many domains, the second and third calls cost the same.

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
