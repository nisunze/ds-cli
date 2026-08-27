# `ds dsgrid` — reference

Tier-4 reference. `ds dsgrid <command> --help` is the contract.
`dsgrid inspect` has its own page: [`dsgrid.inspect.md`](dsgrid.inspect.md).

## Why this domain links instead of calling

`ds-grid-model`, `ds-grid-engine` and `ds-grid-exchange` are pure libraries
with a clean boundary — no ambient state, no process contract, no documented
reason to stay separate. `ds-web/src-tauri` links them; so does `ds`.

The consequence is worth stating because it is visible to a caller: **this
domain has no external dependency at all.** `ds doctor` reports every
`dsgrid` command available on a machine with no sidecar installed and an
empty `PATH`, and `domain_smoke.rs` asserts exactly that. Contrast `ds report`
and `ds solar`, which call binaries and report `unavailable` without them.

## `validate` answers two questions, not one

A `.dsgrid` can be a sound container holding an unsound model, and the two are
fixed in completely different ways. So they are reported apart:

| Field | Question | Failure means |
|---|---|---|
| `container.verified` | does every member match the manifest's byte length, digest, row count and schema fingerprint? | the file is damaged or was written by an incompatible release |
| `model.valid` | is the authored content sound by `ds-grid-model`'s own rules? | the file is intact and the content is wrong |

A damaged container is reported as a **result**, not a refusal — exit 0 with
`container.verified: false` and `model: null`. The caller asked whether the
package is sound; answering "no" is this command working. `model` is null
rather than absent because the model was not judged unsound either; it was not
judged at all.

## `describe` is the engine describing itself

`ds-grid-engine` publishes three catalogs: journaled `commands`, all
`operations`, and `projections`. Each entry carries its parameters, its effect
class, whether it is journaled, and its result type.

Nothing is copied into this repository. The descriptors come from the engine
compiled into this binary, so they cannot be stale relative to what it will
actually do.

The catalog is large, so it is tiered the same way `ds` tiers its own help:

```bash
ds dsgrid describe                       # the operation index
ds dsgrid describe --kind commands       # journaled mutations only
ds dsgrid describe --id create_alignment # one full descriptor
```

Two small translations, both deliberate. The engine spells the effect field
`effect_class`; `ds` reports it as `effect`, because `ds` uses that word for
the same idea everywhere else and a caller should not learn a second one at a
single command. And the three catalogs do not agree on how to spell an id
(`operation_id`, `command_id`, `projection_id`), so `ds` normalizes to `id`.

## Applying one canonical revision

`dsgrid apply` is the one file-writing command in this domain. It consumes the
engine's own `CommandEnvelope`, evaluates its expected authored revision and
model invariants, and writes a new package. It never edits the source and
never overwrites an existing output. Assets and PLS exchange bindings survive
unchanged unless the engine command itself deliberately changes canonical
model state.

Use `--dry-run` first for engineering edits. A successful dry run proves the
envelope addresses the current revision and introduces no new validation
errors; it does not prove that a later PLS-CADD export opens natively.

Read the live command catalog before constructing an envelope:

```bash
ds dsgrid describe --kind commands
ds dsgrid describe --kind commands --id insert_terrain_point_at_station
```

The expected revision is the authored revision returned by the package's
engine session, not the package's monotonic `model_revision`. They are
reported separately in the apply receipt and must never be substituted.

## `project` runs what `describe` lists

`describe --kind projections` published eight projections that nothing outside
the desktop application could invoke. `project` closes that: it opens a
verified package, pins the authored revision, and calls one of them.

```bash
ds dsgrid describe --kind projections --id project_profile --output json
ds dsgrid project --model ./model.dsgrid --id project_profile \
  --param alignment_id=<id> --out ./profile.json --output json
```

The descriptor is the contract in both directions. A `--param` the descriptor
does not declare refuses with `unknown_param`; a required one that is absent
refuses with `missing_param` and names its value type. Neither is a courtesy —
an ignored parameter is a different answer that looks like the one asked for.

**`project_profile` is the clearance answer.** For one alignment it returns the
route nodes, the structures with station/offset/ground/embedment, the terrain
observations, the effective ground segments, and — for every tension section on
that alignment — the solved conductor curve: catenary constant, horizontal and
maximum support tension with their RTS percentages, ruling span, the governing
criterion rule, per-span sag and low point, and the clearance evidence with its
required value, calculated minimum and where the minimum occurs. All of it is
`ds-grid-engine`'s; `ds` adds no number.

### Two bounds, because there are two shapes of result

A projection over a real network is far larger than a terminal or an agent's
context, so stdout is always bounded and `--out` always is not:

| Result | stdout | `--out` |
|---|---|---|
| `Vec<ProjectionRow>` | `--limit` rows (default 50, max 5000) plus `more.withheld` | every row |
| a scene or catalogue | inline when under 256 KiB, otherwise withheld with its byte length | the whole document |

A withheld result is reported, never silently dropped: `more.result_withheld`
with the bytes it would have cost and the flag that would fetch it. The file
`--out` writes carries the same identity block as stdout — model id, package
revision, authored revision, projection, params — so a document found later
says what it is a projection *of*.

`--out` never overwrites. It is the same no-overwrite policy `apply` uses,
from the same module.

### The authored revision, without provoking a conflict

`project` reports `source.authored_revision` — the `rev:<sha256>` an `apply`
envelope must pin. Before this command the only ways to learn it were the
previous apply receipt or a deliberately stale dry run that refused with
`revision_conflict`. Reading a model to find out how to edit it should not
require failing first.

## Making a `.dsgrid`, and exporting one

Classification, planning and conversion are not in this domain. They are
`ds dsgrid-exchange` — see
[`dsgrid-exchange.md`](dsgrid-exchange.md).

The split is by source boundary, not just file extension. The exchange domain
manufactures a canonical package from foreign sources or exports it to a
foreign format. `dsgrid apply` revises one already-canonical package through
the engine's journaled command contract. A reader who only wants model
identity still reaches it without loading exchange planning.

## Ownership

`ds` computes none of this. It reads bytes and calls:

| Command | Owner |
|---|---|
| `inspect` | `ds_grid_exchange::dsgrid::inspect`, `package::unpack`, `ds_grid_model::GridModelSummary` |
| `validate` | `ds_grid_exchange::package::unpack`, `ds_grid_model::validate_snapshot` |
| `describe` | `ds_grid_engine::{describe_commands, describe_operations, describe_projections}` |
| `project` | `ds_grid_engine::GridSession` projection methods |
| `apply` | `ds_grid_engine::GridSession`, `ds_grid_exchange::dsgrid::emit` |

There is no second implementation of the `.dsgrid` format, of model
validation, or of source classification in this repository, and there must not
be one: two readers with two tolerances disagree silently, and the caller
receives a different answer rather than a disagreement.
