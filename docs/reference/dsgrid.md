# `ds dsgrid` — reference

Tier-4 reference. `ds dsgrid <command> --help` is the contract.
`dsgrid inspect` has its own page: [`dsgrid.inspect.md`](dsgrid.inspect.md).

## Two owners, kept separate

`ds-grid-model`, `ds-grid-engine` and `ds-grid-exchange` are pure libraries
with a clean boundary — no ambient state, no process contract, no documented
reason to stay separate. `ds-web/src-tauri` links them; so does `ds` for the
file commands (`create`, `inspect`, `validate`, `describe`, `run`, and `apply`).

The working-copy commands answer from THIS machine. A working copy is a fact
about a machine — a lane, a DS account and a directory beside the local-layer
catalogue — decided by `ds_command_kernel::local_models` and persisted by
`ds-layer-store`: `model list`, `model show`, `model create-local`, `model
import-external` and `model set-active` need no application, project or
window (since 2026-09-18). `model prepare-project` (since 2026-09-20) reads the
CLI-selected project's governed MV heads under the native credential and
answers which of them this machine holds as exact working copies; with
`--download-missing` it fills the rest, one verified download at a time, each
registered as a project-pinned copy and never activated. `publish-version`
with `--path` publishes exact bytes through the native owner; without it, it
still publishes the paired Desktop's own browser-held working copy through
the bridge — the one door left in this domain.

This split is visible in availability. The native file commands remain
available without a Desktop, sidecar, or populated `PATH`; the working-copy
and project commands are honestly unavailable in a build with no digest-pinned
release catalog. No command in this domain calls a sidecar process.

These file commands provide native model creation, engineering reads/solves,
revision-gated mutation and persistence. They do not make the paired model
catalogue, active-model selection or project publication headless; those
commands retain their explicitly declared authority below.

## Local acquisition, activity, and publication

These words are deliberately not interchangeable:

| Command | Meaning | Authority |
|---|---|---|
| `ds dsgrid model list` | List this machine's working copies and the active one. | none (`--lane`, `--account`) |
| `ds dsgrid model create-local` | Create one empty working copy and open it as active. | none |
| `ds dsgrid model import-external` | Acquire one external `.dsgrid`; it does not become active. | none |
| `ds dsgrid model set-active` | Open one existing working copy as the active one; idempotent when already active. | none |
| `ds dsgrid model prepare-project` | Show which exact governed MV heads of the selected project this machine holds and, with `--download-missing`, fill the rest as project-pinned working copies. | headless_project |
| `ds dsgrid publish-version` | Register one immutable revision in a project's catalogue; never changes local activity. | project + `--yes` |

The local commands never accept a project. Publication never accepts arbitrary
model bytes: it names an absolute `.dsgrid` path with an explicit `--project`,
or an opaque local model of the paired Desktop, which supplies its own selected
project.

`prepare-project` is the headless form of the desktop's project MV cache:
a head is *held* when a working copy pinned to exactly that project, model,
revision and digest exists, or when any copy's bytes are that digest; a copy
edited since it was pinned is not the head. The receipt names each head's
`local_id` and `bytes` when held, `downloaded` ids for this run, and
`complete`. Over 1000 heads it refuses `grid_project_inventory_unbounded`
rather than fold a partial listing as complete; a head whose bytes do not
match its declared digest is `grid_project_head_unverified` and nothing is
registered for it.

```bash
ds auth project use --lane canary --project <exact-id>
ds dsgrid model prepare-project --lane canary --output json
ds dsgrid model prepare-project --lane canary --download-missing --output json
ds dsgrid model list --lane canary --account <uid>       # the copies, origin "project"
```
PLS-CADD workspaces and `.bak` files remain under `ds dsgrid-exchange inspect`,
`plan`, and `convert`; there is no second conversion verb here.

## The link to a PLS-CADD workspace

```bash
ds dsgrid model link --model local-<id> --workspace "/srv/pls/Nyamagabe" --account <uid>
ds dsgrid model show --model local-<id> --account <uid>
```

A working copy converted from a PLS-CADD folder knows the bytes it came from
— the package preserves the original member tree — but not where they live.
`link` records where, on the catalogue row, as `pls_source`: the folder, the
exchange digest of its member tree (the digest `ds dsgrid-exchange inspect`
prints), the PLS-CADD program version (`16.81`), every member family's
version (`DON 57`, `NUM 14`, `CRI 94`, `FEA 15`, `STRUCT 13`, `XYZ 5`, `TIN
5`, `PPS 57` …), the member count and when. The folder must digest to the
workspace the package was imported from; any other folder is
`workspace_not_this_package`, because a sync into it would write edits
computed against a different baseline. Relinking replaces the link.

`import-external` links automatically when the package sits beside the
`exchange-report.json` `convert` wrote, the report names one PLS-CADD folder
source with its pinned digest, the package preserved a tree digesting to that
pin, and the folder still digests to it now. Anything short of that leaves
the copy unlinked with the reason in `auto_link`, and `link` is the explicit
act. `list` prints the link under the row; `show` prints the row, the package
identity as its bytes declare it now, and the link in full.

The link is what `ds dsgrid-exchange sync` writes into; see
[`dsgrid-exchange.md`](dsgrid-exchange.md).

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

Optimum structure spotting uses the same mapless path. Discover the exact
request first, then provide only authored ids and bounds from that model
revision:

```text
ds dsgrid describe --kind operations --id plan_optimum_spotting --output json
ds dsgrid run --model model.dsgrid --operation plan_optimum_spotting --params spotting.json --output json
```

A successful proposal includes the objective, structures, spans, per-span and
per-structure engineering evidence, bounded rejected alternatives,
infeasibility when applicable, and atomic canonical commands. A planning
refusal keeps a stable reason at `error.detail.refusal.code` with the exact
missing or rejected facts under `error.detail.refusal.detail`; callers do not
need to parse the human-readable `error.detail.engine` sentence. Rendering the
proposal on a map is optional and contributes no engineering authority.

Every response identifies the package bytes and authored revision that were
read, reports `staged: false` and `persisted: false`, and recursively bounds
large arrays with exact `more.truncated` receipts. `ds dsgrid validate` always
reports the authored revision. The cheap inspect path exposes it on demand
with `--include authored-revision`, which deliberately decodes the model.

## Applying one canonical revision

`dsgrid apply` revises an existing model file. It consumes the
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

## Typed edits of a working copy: `structure describe|retype`, `report structures`

The first typed mutations over the engine (program contract 01 §2, landed
2026-09-20 for the Nyamagabe resubmission). They share one plumbing,
`ds-cli-dsgrid::mutation`, and therefore one vocabulary:

| Input | Meaning |
|---|---|
| `--model <local-id>` | one of this machine's working copies (`ds dsgrid model list`); the edit becomes its **next revision in place**, same id, package revision +1, `head_revision` updated on the row |
| `--package <path> --out <path>` | an immutable `.dsgrid` file; a new file is written, never over the source |
| `--revision <rev>` | the authored head you observed (`ds dsgrid model show --model <id>` prints it); a moved head refuses `revision_conflict` |
| `--dry-run` / `--yes` | exactly one: evaluate against the exact head and write nothing, or write |

The receipt names the engine operation(s) with the SHA-256 of their published
descriptors, `source_revision → resulting_revision`, the counts touched, the
warnings, the working copy's `pls_source` link (contract 02; null until
`model link`) and `pls_members_affected` as `TYPE VERSION` from that link.

```bash
ds dsgrid model list --account <uid>                       # ids and heads
ds dsgrid model show --model local-…                       # the live head to pin
ds dsgrid structure describe --model local-… --structure 230     --text "W045S-A0101-11 MV H-Poles Assembly, 12 m wooden, 10–60°, 2 stays" --dry-run
ds dsgrid structure describe --model local-… --structure 230 --text "…" --yes
ds dsgrid structure retype --model local-… --structure 3 --type j-w-60d-S190.012 --dry-run
ds dsgrid structure retype --model local-… --from-finding structure_type_not_allowed     --skip 346 --skip 446 --type j-w-60d-S190.012 --dry-run     # then --yes: ONE revision
ds dsgrid report structures --model local-… --out structures.csv     # or .xlsx
ds dsgrid report structures --model local-… --only-findings --output json
```

`--structure` takes a structure id (`str-…`) or, when unique, the engineering
number the sheet prints; `--type` a type id (`st-…`) or the exact library name
(`j-w-60d-S325.014`). An engineer types what they read; the receipt carries
the id.

**What the engine decides.** `describe_structure` stores one trimmed line of
at most 500 characters on the placed structure (`StructureRow.description`,
distinct from the library type's description; contract 02 exports it into the
DON 57 structure record on sync). `retype_structure` re-binds every strung
support of the structure to the new type's attachment point with the same set
label and slot — what PLS-CADD keeps on a structure-file substitution — and
refuses by name when the new type lacks a strung set (a T-off's `30kV [set 4]`
cannot become a plain H-pole: `--skip` it or choose a type carrying the set).
Before a retype is written the REG v7 angle-pole rule is evaluated: the dry-run
receipt lists `findings {cleared, remaining, created}` and a write that would
leave or create `structure_type_not_allowed` (a single pole carrying
10° ≤ |line angle| < 60°) is refused with that code.

**The structure list** (`report structures`, contract 04 §5) is the engine's
`report_structures` read: one row per placed structure — id, number,
alignment, station, line angle from the model's own alignment geometry (right
turn positive, as PLS-CADD prints it; the native source's recorded angle beside
it), structure type, the structure's own description and the library's, pole
family / material / height / class / stays parsed from the type name, the
assembly drawing number when a description carries one, REG Table 14
foundation depth/width by pole height, and the findings with rule id, source
clause and an empty reason slot. CSV always (UTF-8 BOM); XLSX through the
stack's shared `ds-io` workbook writer. The receipt carries the standard's
`{schema, version, issued, digest}`, the `assumptions[]` the engine evaluated
in the standard's silence (`assumed: true`: the JSON carries the angle-pole
classes but not the 10°–60° band, which is declared in code from EDCL drawing
-04 "Not recommended"), `verification_level: proposal`, and a bounded page of
rows (`--limit`, `more.withheld`); the file is whole.

**Packages written before 2026-09-20** predate the `description` column and
still open: the column is additive, every description reads as absent, and
`inspect`, `validate` and `model show` name the member under
`prior_schema_members[] {member, table, columns, current_columns}`
(`tables/structures.arrow`, 15 of 16). The first write (`structure describe`,
`structure retype`, any revision) carries the current schema; nothing is
re-converted. `package_decode_failed` is kept for a package that is damaged
or carries a table schema this build does not decode at all.

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
| `create` | `ds_grid_exchange::create_blank_model` |
| `inspect` | `ds_grid_exchange::dsgrid::inspect`, `package::unpack`, `ds_grid_model::GridModelSummary` |
| `validate` | `ds_grid_exchange::package::unpack`, `ds_grid_model::validate_snapshot` |
| `describe` | `ds_grid_engine::{describe_commands, describe_operations, describe_projections}` |
| `run` | the operation selected from `ds_grid_engine::operation_descriptors` and its typed native engine API |
| `apply` | `ds_grid_engine::GridSession`, `ds_grid_exchange::dsgrid::emit` |
| `model list/show/create-local/import-external/set-active` | `ds_command_kernel::local_models` over `ds_layer_store::local_models` (this machine's catalogue); `show` opens the package with `ds_grid_engine::GridSession` |
| `structure describe/retype` | `ds_grid_engine::GridSession::apply_transaction_at_head` (`describe_structure`, `retype_structure`), `ds_grid_engine::evaluate_structure_type`, `ds_grid_exchange::dsgrid::emit`, `local_models::Op::Revise` |
| `report structures` | `ds_grid_engine::report_structures` (+ `structure_rules::load_standard`), `ds_io::layers_to_xlsx` |
| `publish-version` | paired Desktop `dsgrid.model.publish`, composing its existing project version flow |

There is no second implementation of the `.dsgrid` format, model validation,
source classification, browser-local session state, or project publication in
this repository, and there must not be one: two owners with two tolerances or
two notions of "active" disagree silently, and the caller receives a different
answer rather than a disagreement.

## A model's structures as a task's geometry

`ds pm task create --geometry-from dsgrid:local-<id>:structure:74,76,77` and
`ds pm task geometry set --from …` read a working copy from this machine's
catalogue (or a `.dsgrid` named with `--package`) and give a Project Work
task the geometry of the named structures or alignment, with one `ds_object`
link per object. The engine's own projection supplies every position; the
kernel resolves and shapes; nothing in this domain is computed twice.
`docs/reference/pm.md` has the grammar and the shaping rules.
