# `ds dsgrid` — reference

Tier-4 reference. `ds dsgrid <command> --help` is the contract.
`dsgrid inspect` has its own page: [`dsgrid.inspect.md`](dsgrid.inspect.md).

## Two owners, kept separate

`ds-grid-model`, `ds-grid-engine` and `ds-grid-exchange` are pure libraries
with a clean boundary — no ambient state, no process contract, no documented
reason to stay separate. `ds-web/src-tauri` links them; so does `ds` for the
file commands (`inspect`, `validate`, `describe`, and `apply`).

The local-model commands have a different owner. A browser-local DS Grid model
is a live worker session and durable application store, not a file the CLI can
open. `model list`, `model create-local`, `model import-external`, and `model
set-active` therefore ask the paired Desktop through one named operation each.
They are project-independent; a projectless paired session is valid. Only
`publish-version` needs project authority, because it registers one immutable
revision in the paired session's selected project.

This split is visible in availability. The four file commands remain available
without a Desktop, sidecar, or populated `PATH`. The local-model family needs a
paired Desktop, and publication additionally needs its signed-in project. No
command in this domain calls a sidecar process.

## Local acquisition, activity, and publication

These words are deliberately not interchangeable:

| Command | Meaning | Authority |
|---|---|---|
| `ds dsgrid model list` | List bounded browser-local model identities and the active one. | paired Desktop |
| `ds dsgrid model create-local` | Create one empty local model; the application opens it as active. | paired Desktop |
| `ds dsgrid model import-external` | Acquire one external `.dsgrid`; it does not become active. | paired Desktop |
| `ds dsgrid model set-active` | Open one existing local model in Profile; idempotent when already active. | paired Desktop |
| `ds dsgrid publish-version` | Register one immutable revision in the selected project's catalogue; never changes local activity. | project + `--yes` |

The local commands never accept a project. Publication never accepts arbitrary
model bytes or a project id: it names an opaque local model or an absolute
`.dsgrid` path, and the paired Desktop supplies its own selected project.
PLS-CADD workspaces and `.bak` files remain under `ds dsgrid-exchange inspect`,
`plan`, and `convert`; there is no second conversion verb here.

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

## Running native non-mutating operations

`ds dsgrid run` executes the read, solve, and propose operations published by
the native engine compiled into the CLI. It never admits journaled mutations,
imports, or exports, and never writes the source `.dsgrid` package.

Discover the exact operation and parameter contract before invoking it:

```text
ds dsgrid describe --kind operations --id project_profile
ds dsgrid run --model model.dsgrid --operation project_profile --params profile.json --output json
```

Every response identifies the package bytes and authored revision that were
read, reports `staged: false` and `persisted: false`, and recursively bounds
large arrays with exact `more.truncated` receipts. `ds dsgrid validate` always
reports the authored revision. The cheap inspect path exposes it on demand
with `--include authored-revision`, which deliberately decodes the model.

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
| `run` | the operation selected from `ds_grid_engine::operation_descriptors` and its typed native engine API |
| `apply` | `ds_grid_engine::GridSession`, `ds_grid_exchange::dsgrid::emit` |
| `model list/create-local/import-external/set-active` | paired Desktop `dsgrid.model.*` operations |
| `publish-version` | paired Desktop `dsgrid.model.publish`, composing its existing project version flow |

There is no second implementation of the `.dsgrid` format, model validation,
source classification, browser-local session state, or project publication in
this repository, and there must not be one: two owners with two tolerances or
two notions of "active" disagree silently, and the caller receives a different
answer rather than a disagreement.
