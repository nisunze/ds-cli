# Registering a project DS Grid model

A **project DS Grid model** is a project's own copy of a network: a mutable
head row in the project catalogue plus a chain of immutable revisions, each
one an exact `.dsgrid` package stored by digest. It is not the global
engineering library, and this is the distinction the family exists to keep:

| | `ds library …` | `ds dsgrid model …` |
|---|---|---|
| scope | one immutable global version, shared by every project | one project's own model |
| identity | a library release / catalogue example | a model head + revision chain |
| who may write | catalogue publishers | project members with the model capability |
| ds-brain | grid catalogue | `POST /grid/models` |

`ds dsgrid model` is the one canonical place a project acquires a model, with
three sources for the same act:

| Command | Source |
|---|---|
| `ds dsgrid model create` | nothing — a network that will be drawn |
| `ds dsgrid model import` | a `.dsgrid` package the operator already verified |
| `ds dsgrid model convert` | a PLS-CADD workspace or `.bak` |

There is a fourth source and it deliberately stays where it is:
`ds library global fork-example` copies one governed catalogue example into a
project model. That command belongs to library governance because what it
authorizes is reading the global catalogue; the project model is its result,
not its subject. It is also **the only one of the four that works today**.

## Status: all three refuse closed

Every verb refuses with `project_model_registration_unsupported`, at the
availability gate, so `ds capabilities dsgrid.model.import` reports it without
running anything.

That is not a gap in ds-brain. The whole contract is already there:

```
POST /grid/models        internal/handlers/project_grid_models.go
  read   list_models · get_model · list_versions · get_version · list_exports
  write  start_upload · create_version · fork_example · delete_model · publish_export
```

Writes are gated on the project capability (`gridModelVersionCapability`,
`gridModelApproveCapability` for an approval decision, `gridModelDeleteCapability`
for deletion), and `create_version` takes an `expected_head_revision_id` so a
revision can never silently replace a head the caller did not see.

What is missing is a **route from `ds` to it**. The paired application
publishes exactly one grid-model operation on its closed CLI bridge —
`catalog.fork-example` — and `ds-client-core` publishes none. So the choice
was between guessing and refusing, and guessing would have meant a generic
HTTP client, an ambient service account, or reading the project directly. Each
of those is something this repository exists to prevent.

## What has to exist first

Two desktop bridge operations, each registered in the three places every CLI
bridge operation is registered.

### 1. `grid.model.start_upload`

| Where | What |
|---|---|
| `src-tauri/src/cli_bridge.rs` | `"grid.model.start_upload"` in `CLI_OPERATIONS` |
| `src/lib/desktop/cli-bridge.ts` | one `case "grid.model.start_upload":` |
| `src/lib/desktop/cli-catalog.ts` | `'grid.model.start_upload': ['project_id', 'upload']`, calling `startGridModelUpload` and `uploadGridModelBytes` from `$lib/grid/models-api` |

`upload` carries `model_id`, `digest`, `byte_length` and optionally `purpose`
and `content_type`, exactly as `services.StartProjectGridUploadRequest`
declares them. The bytes are read from a native path the way
`design.upload.inspect` already reads one, through `prepareNativeSource` —
package bytes must never cross the bridge as an argument.

The reply is ds-brain's `StartProjectGridUploadResult`: `artifact`,
`already_exists`, `session_uri`, `max_chunk_size`. `already_exists` is the
one that matters to `ds`: a package whose digest is already stored is
registered without re-uploading, and the CLI should report that rather than
implying it transferred anything.

### 2. `grid.model.create_version`

| Where | What |
|---|---|
| `src-tauri/src/cli_bridge.rs` | `"grid.model.create_version"` in `CLI_OPERATIONS` |
| `src/lib/desktop/cli-bridge.ts` | one `case "grid.model.create_version":` |
| `src/lib/desktop/cli-catalog.ts` | `'grid.model.create_version': ['project_id', 'version']`, calling `createGridModelVersion` |

`version` is `services.CreateProjectGridVersionRequest` — `model_id`,
`revision_id`, `expected_head_revision_id`, `display_name`, `description`,
`model_kind`, `lifecycle`, `model`, `model_schema_version`, `engine_version`,
`reason`, `design_stage_id`, `detail_level_id`, `approval`, `validation`, and
optionally `operation_summary`, `composition_sources`, `migration_source`.

Two of those fields are how each CLI verb identifies itself, and they should
not be inferred by the adapter:

- `migration_source` — `{ kind: "pls_cadd", reference: <workspace or .bak
  name> }` for `convert`, absent for `create` and `import`.
- `validation` — the summary from `ds dsgrid validate`, so a package's
  soundness is recorded by the tool that actually checked it.

`approval` stays `{ status: "draft" }` from the CLI. Registering a model and
approving one are different capabilities in ds-brain
(`gridModelApproveCapability`), and a CLI verb that could do both would make
one confirmation authorize two decisions.

### Then, in this repository

1. `DESIGN`-style `BridgeOp` constants in `ds-cli-dsgrid`, added to a
   `BRIDGE_OPS` slice, and a parity block in `crates/ds/tests/bridge_parity.rs`
   next to the catalog one — `ds-cli-dsgrid` currently links only engine
   crates, so this is also where it gains a `ds-cli-desktop` dependency.
2. `registration_availability()` in `crates/ds-cli-dsgrid/src/model.rs`
   becomes `ds_cli_desktop::ops::paired_availability`, and the three handlers
   assemble their typed request instead of refusing. The declared inputs do
   not change — that is what declaring them now is for.
3. The `model_not_found`, `source_not_found` and `expected_head_required`
   refusals become reachable; the paired-session refusals join each command's
   list; `project_model_registration_unsupported` is retired.
4. `ds dsgrid model convert` calls `ds-cli-dsgrid-exchange` for the conversion
   half rather than repeating it. That half already works today, locally, with
   no project and no principal.

### What must not be built instead

- A generic `POST` helper reachable from a command. `ds-client-core` is a
  closed set of fixed method/path/body contracts; adding a grid-model call
  means extending the profile schema, transport trait, response decoder and
  package digest together.
- An ambient service account or ADC path to Firestore. `ds` has no hidden
  privilege; a project id is not proof of anything.
- Reading the desktop's IndexedDB for the model list. That store is an
  implementation detail.

## Until then

The local half of every route already works, needs no project and no
principal, and is what the refusals route callers to:

```bash
ds dsgrid validate --model ./karongi.dsgrid --output json     # prove a package
ds dsgrid-exchange inspect --source ./Karongi --output json   # classify a workspace
ds dsgrid-exchange plan    --source ./Karongi --output json   # what a conversion would do
ds dsgrid-exchange convert --source ./Karongi --out ./karongi.dsgrid
```

Register the result in DS GridDesign, or — when starting from a governed
catalogue example rather than a file — use the path that is already wired:

```bash
ds library global fork-example --payload '{"project_id":"…","fork":{…}}' --yes
```
