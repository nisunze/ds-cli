# CLI ergonomics recommendation

**Date:** 2026-08-24  
**Status:** recommendation; not an active compatibility contract  
**Scope:** discovery, configuration, list behavior, output shaping, command naming,
completion, and long-running operations

## Verdict

`ds` has the difficult foundations right. Its structured envelope, meaningful
exit classes, capability descriptors, preflight diagnostics, progressive
discovery, and strict flag validation are stronger and more machine-usable than
the equivalent surfaces commonly available from `gcloud`.

The remaining shortcomings are primarily convenience and consistency gaps.
They matter because `ds` serves both people and coding agents, and several of
them sit directly on the discovery path every caller uses.

This work must preserve the existing strengths while improving the everyday
surface. It must not weaken progressive disclosure, typed refusals, bounded
results, explicit authority, or the one-declaration command contract.

## Foundations to preserve

1. Every success and failure uses the versioned response envelope. Errors keep
   their stable class, code, retryability, remedy, and next-step fields.
2. Exit codes continue to identify invalid input, unavailable dependencies,
   authorization failure, conflict, execution failure, and internal defects.
3. `ds capabilities` remains the authoritative machine-readable command tree.
   Descriptors continue to declare effect, authority, availability,
   confirmation requirements, typed inputs, refusals, and runnable examples.
4. `ds doctor` remains a side-effect-free preflight. It must not start an
   engine or make a network call merely to explain availability.
5. Unknown and out-of-scope flags continue to fail closed. New convenience
   flags must be declared at the narrowest tier that owns them.

## Findings

### 1. Capability search ranking is too flat

`ds capabilities --search design` currently returns many alphabetically sorted
matches with the same `terms_matched` score. Generic commands can rank above
the exact `map.design.*` family, and the default limit can hide relevant design
and Solar commands completely.

This is the highest-cost defect because capability search is an early step in
the documented agent workflow.

### 2. List commands do not share one paging and filtering contract

Observed paging conventions include:

| Command family | Current paging |
|---|---|
| `pls pole-capacity read` | `--offset` and `--limit` |
| `work task list`, `work record list` | `--page` and `--limit` |
| `map design list` | `--limit` only |
| `solar portfolio list` | none |

Filtering is similarly command-specific. A caller must relearn traversal and
filtering for each collection, and some collections cannot be fully traversed.

### 3. JSON is structured, but output cannot be projected

The current `--output human|json` and `--pretty` surface is reliable but forces
shell callers to invoke `jq` for common scalar, table, and CSV projections.
This is the largest remaining scripting ergonomics gap.

### 4. Repeated desktop selection has no persistent configuration layer

Commands that use the paired application repeatedly require
`--desktop-descriptor`. This is particularly costly when more than one desktop
profile is running. `DS_SOLAR_BIN` already provides an environment override for
one boundary, while an equivalent `DS_DESKTOP_DESCRIPTOR` override is not
consumed consistently. There are no named CLI profiles or `ds config` surface.

### 5. The command tree has no generated shell completion

With a large, typed command tree, the absence of `ds completion` creates a
daily discovery tax. The capability registry already contains nearly all data
needed to generate completion without maintaining a second command model.

### 6. Long-running execution is domain-specific

Solar exposes its own `start`, `progress`, `result`, and `cancel` lifecycle
while command descriptors still describe execution simply as synchronous. A
second long-running domain would likely invent another lifecycle unless the
CLI defines a common operations contract first.

### 7. Verbs are inconsistent at shallow command depths

Deep paths generally follow the useful noun-path, verb-last shape, but similar
actions currently use several words:

- single-item reads: `inspect`, `describe`, `read`, `view`, `status`, `progress`;
- deletion: `remove` and `delete`;
- creation: `draw` and `create`;
- action-like nouns without a terminal verb: `outliers`, `points-along`,
  `random-points`, `section-orientation`, `reference-closure`, and
  `compare-don`.

`ds report export` and `ds solar report export` also form a semantic collision
for humans and prefix-oriented discovery even though their full paths differ.

## Recommended work order

### Priority 1 — rank capability search semantically

Score matches instead of sorting equal matches alphabetically. At minimum:

1. exact command identifier;
2. exact identifier token or path segment;
3. identifier prefix;
4. summary token;
5. purpose and remaining descriptor text.

Use deterministic tie-breaking and retain bounded results. Search evidence
should explain the score without inflating the tier-1 response.

Acceptance criteria:

- `--search design` ranks `map.design.*` ahead of unrelated commands whose
  prose merely contains “design”;
- the default limit retains the most relevant command families;
- tests cover exact ID, path-segment, summary, multi-token, and stable-tie
  ranking;
- search remains a cheap index and does not inline full descriptors.

### Priority 2 — make desktop selection configurable

*Partly landed, verified 2026-08-28:* `--desktop-descriptor` and
`DS_DESKTOP_DESCRIPTOR` both exist and their precedence is declared — the flag,
then the variable, then automatic discovery, with the variable a default for
the flag rather than an override of it. Named profiles and a `ds config`
surface do not exist; the rest of this item is unchanged.

Support the same descriptor through a consistent precedence chain:

1. explicit `--desktop-descriptor`;
2. `DS_DESKTOP_DESCRIPTOR`;
3. selected named profile;
4. unambiguous automatic discovery.

Add a bounded `ds config` surface for inspecting and setting non-secret client
preferences. Configuration must never contain session credentials, pairing
secrets, or project authority. Ambiguous discovery must remain a typed refusal
when no explicit selection exists.

### Priority 3 — adopt one list contract

All list commands should implement the same total bound and continuation
interface: `--limit` plus `--page`. Every collection must be traversable, and
every truncated response must continue to report `more` explicitly.

Filtering should converge on one declared expression language rather than
growing unrelated flags indefinitely. Existing high-value filter flags may
remain as aliases during migration, but they must compile to the same typed
filter representation.

### Priority 4 — add bounded output projections

Add a `--format` projection layer with two initial forms:

- `value(field.path)` for one scalar or a repeated scalar column;
- `table(field,other.path,...)` for human-readable tabular output.

CSV can follow once escaping and repeated-value behavior are contracted.
Projection happens after a command returns its typed envelope and must not
change execution, authority, paging, or the underlying JSON contract. Invalid
field paths fail as structured invalid input rather than producing blanks.

### Priority 5 — normalize verbs with a deprecation window

Define a closed verb vocabulary before adding more commands. Prefer one
canonical term for equivalent operations, including one-item reads and
deletion. Keep old spellings as explicit aliases for one release, advertise
the canonical path in `next`, then remove aliases in the next declared
contract revision.

Do not mechanically rename domain terms that carry different meaning. In
particular, `status` and `progress` may remain distinct lifecycle views if the
common operations contract defines them as such.

### Priority 6 — generate shell completion from command declarations

Add `ds completion <shell>` for the supported shells. Generate domains,
commands, flags, enums, and switch behavior from the same `Command` values used
by help and capabilities. Completion must not introduce a second registry or
probe unavailable engines.

### Priority 7 — define a common operations collection

Before adding another asynchronous domain, define a uniform operations
surface for start, status/progress, result, and cancellation. Domain commands
may initiate work, but lifecycle inspection should use stable identifiers and
one envelope shape across domains.

## Compatibility and sequencing

Search ranking, environment/config resolution, and paging completion can be
introduced without changing existing command meanings. They should land first.

Output projections add a new global presentation contract and therefore need
their own tests and documentation. Verb normalization changes discoverable
paths and requires aliases plus an announced removal window. Neither should be
combined with unrelated domain expansion.

This recommendation does not authorize implementation by itself. Each change
must still satisfy the active discovery, output, process-boundary, refusal, and
context-budget contracts.
