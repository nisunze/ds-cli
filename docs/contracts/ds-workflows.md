# DS Workflows — the data and geographic processing contract

**Status:** specification, owner direction 2026-10-07. No code yet. Implementation
order and proof gates are in [`backlog/WORKFLOWS-TODO.md`](../../backlog/WORKFLOWS-TODO.md).
Open owner questions are listed in §14 and must not be guessed in code.

## 1. Intent

A **workflow** is the one preferred way to clean, transform, combine, enrich and
deliver data and geography in DS. It is a declarative, versioned document of
typed steps over named inputs, run by Rust engines through `ds` and its MCP
surface — the way Dataprep runs a recipe over a dataset.

- **Data Factory becomes a workflow builder.** Every cleaning or table action a
  person takes in Data Factory appends a step to a *temporary* workflow; the
  preview is that workflow run on a sample; "apply" executes it; "save" promotes
  it to a stored workflow. Data Factory owns no transformation logic of its own.
- **Agents compose workflows instead of scripting.** An agent that needs a
  buffer, clip, join, dedupe, rename or domain mapping writes or edits a
  workflow and runs it through `ds`. It never patches data with geopandas,
  pandas, Polars, shapely or ad-hoc JavaScript (owner ruling 2026-09-23: no
  Python, ever).
- **One implementation per operation.** Every workflow operation is an
  existing Rust engine function behind a command descriptor. A single
  `ds data vector buffer` call and a workflow `data.vector.buffer` step run the
  same code with the same contract; neither has a private copy.

### Non-goals

- No embedded scripting language (Python, JavaScript, SQL text, Lua). Steps are
  typed operations with typed arguments. The expression surface (§7) is a
  closed, structured grammar.
- No scheduler, trigger or background runner. A workflow runs when a person or
  agent runs it (owner ruling: nothing runs on its own initiative).
- No new cloud orchestration service in v1. Cloud-side operations already
  behind `ds` (spatial reads, project datasets) appear as inputs, not as a
  remote executor.
- No UI-first design. The CLI/MCP contract lands and is proven first; Data
  Factory adapts afterwards (owner ruling 2026-09-13: kernel first, driven through CLI and MCP).

## 2. Vocabulary

| Term | Meaning |
| --- | --- |
| **Workflow document** | JSON, schema `ds.workflow/v1`: parameters, inputs, steps, outputs. |
| **Step** | One operation invocation: `id`, `op`, `op_version`, `in` (named frame references), `args`. |
| **Operation (op)** | A registered, versioned transform with a command-descriptor contract (§5). |
| **Frame** | The value flowing between steps: a typed table, optionally with one geometry column (§4). |
| **Source** | A named input that produces a frame (file, project layer, dataset cache, spatial result…). |
| **Sink** | A named output that consumes a frame and has an effect (file, local layer, staged design, asset). |
| **Draft** | An editable, unsaved workflow (temporary; Data Factory sessions create these). |
| **Revision** | An immutable saved workflow identified by its content digest. |
| **Plan** | The validated, fully resolved workflow plus input pins, schema per step, estimates, authority and cost; hashed. |
| **Run receipt** | The record of one execution: digests, parameter values, per-step counts and refusals, outputs. |

## 3. The workflow document — `ds.workflow/v1`

```json
{
  "schema": "ds.workflow/v1",
  "name": "pole-setback-parcels",
  "title": "UPI parcels within a setback of new LV poles",
  "description": "Buffer new poles, keep parcels the zone touches, count them per village.",
  "params": {
    "radius_m": { "type": "number", "min": 0.01, "max": 100000, "default": 30, "unit": "m" },
    "transformer": { "type": "string", "required": true }
  },
  "inputs": {
    "poles": {
      "source": "project.layer",
      "args": { "layer": "lv_poles", "transformer": { "param": "transformer" } }
    }
  },
  "steps": [
    { "id": "new_poles", "op": "table.filter", "op_version": 1,
      "in": { "frame": "poles" },
      "args": { "where": { "column": "status", "op": "equals", "value": "new" } } },
    { "id": "zone", "op": "data.vector.buffer", "op_version": 2,
      "in": { "source": "new_poles" },
      "args": { "radius_m": { "param": "radius_m" }, "segments": 8 } },
    { "id": "parcels", "op": "data.parcels.query", "op_version": 1,
      "in": { "area": "zone" }, "args": {} },
    { "id": "per_village", "op": "table.aggregate", "op_version": 1,
      "in": { "frame": "parcels" },
      "args": { "group_by": ["village"], "measures": [{ "fn": "count", "as": "parcels" }] } }
  ],
  "outputs": {
    "zone_layer": { "from": "zone", "sink": "map.local", "args": { "name": "Pole setback" } },
    "counts":     { "from": "per_village", "sink": "file", "args": { "format": "xlsx", "path": "./setback-parcels.xlsx" } }
  }
}
```

Rules:

1. **Steps form a DAG.** `in` values name an input, or an earlier step id. A
   step may consume several frames (`source`/`against`, `frame`/`mask`,
   `left`/`right`). Cycles, dangling references and unused steps are refusals,
   not warnings.
2. **Arguments are typed literals or parameter references** —
   `{ "param": "name" }`. There is no string interpolation.
3. **`op_version` is mandatory in a saved revision.** A draft may omit it; the
   plan resolves and records it. A revision never silently picks up a newer
   op contract.
4. **Effects live only in sinks.** Steps are pure transforms of frames.
   Reading a project layer or a cloud dataset is an input with an authority,
   never a side effect inside a step.
5. **Canonical JSON.** Key order, number formatting and default elision follow
   one canonicalisation so that the digest (§9) is stable across hosts.

## 4. Frames

A frame is a typed table carried as Arrow record batches (`ds-columnar`), with
at most one geometry column held as WKB.

```json
{
  "columns": [
    { "name": "pole_id", "type": "string", "nullable": false },
    { "name": "status",  "type": "string", "nullable": true },
    { "name": "height_m","type": "float64","nullable": true }
  ],
  "geometry": { "column": "geometry", "classes": ["Point"], "crs": "EPSG:4326" },
  "rows": { "exact": 1240 }
}
```

- **Types:** `string`, `int64`, `float64`, `bool`, `date`, `timestamp`,
  `json`. These are the existing table data types (`ds-geo::table::TableDataType`)
  plus the columnar contract's types; no third type system is introduced.
- **Geometry** is WGS-84 lon/lat unless a step declares a projected CRS. An op
  that needs metres states whether it is geodesic or projected; a frame never
  changes CRS implicitly. Admission (`ds-geo-lite::admission`) runs at every
  source, and repairs or rejections are counted, never silent.
- **Row counts** are `exact`, `estimated` or `unknown` in a plan, and always
  `exact` in a receipt.
- **Schema propagation is static.** Every op declares how its output schema
  follows from its input schemas and arguments, so `ds workflow plan` reports
  the schema at every step without reading data. A step that names a column
  absent upstream is refused at plan time.

## 5. Operations

### 5.1 The op descriptor

Every op uses the canonical command descriptor (`ds capabilities <id>`), with
four additions:

| Field | Meaning |
| --- | --- |
| `frames_in` | Named frame slots and the geometry classes / columns each requires. |
| `schema_out` | The declared schema transfer (adds, drops, renames, retypes, geometry class out). |
| `placement` | `portable` (ds-geo-lite / kernel; runs on every host incl. WASM), `engine` (ds-network heavy compute), `desktop` (needs the paired Desktop), or `cloud` (a governed cloud read). |
| `row_local` | `true` when the op can stream chunk by chunk; `false` when it needs the whole frame (sort, dedupe, aggregate, overlay). |

Effect, authority, confirmation, refusals and contract version keep their
existing meaning. An op that is also a standalone command has **the same id**:
`data.vector.buffer` is both. A workflow-only op (e.g. `table.filter`) is still
discoverable through `ds capabilities` and `ds workflow ops`.

### 5.2 v1 catalogue — grounded in existing engines

The catalogue below maps each op to the Rust that already does the work.
"New" marks the few with no current implementation; they are added only when a
real workflow needs them (YAGNI).

#### Sources

| Source | Existing behaviour |
| --- | --- |
| `file.geojson`, `file.geoparquet` | `ds-columnar`, `ds-geo-lite::geojson` |
| `file.table` (CSV/XLSX with coordinate detection) | `ds data inspect` / `ds data convert` |
| `project.layer` | project layer readers behind design/survey downloads |
| `project.cache` | `ds data project-cache query` |
| `cloud.spatial` | a `ds data spatial plan` file; its billing guard folds into the workflow plan |
| `reference.admin_bounds` | `ds data admin-bounds read` |
| `inline.geojson` | `--source-json` shape of the vector commands |

#### Table ops — from `ds-geo::table` and `ds-cleaning`

| Op | Existing engine |
| --- | --- |
| `table.select`, `table.drop`, `table.rename` | `ds-cleaning` column plans (`excluded`, `renames`) |
| `table.cast` | `TableDataType` conversions |
| `table.add_column` | `ds-geo::table::add_column` |
| `table.calculate` | `calculate_field` — `Constant`, `Copy`, `Concat`, `RowNumber`, `LengthMeters` |
| `table.replace` | `count_replace_matches` / `collect_replace_updates` (scoped literal/regex) |
| `table.clean_values` | `ds-cleaning` `cleanData`, `valueReplacements` |
| `table.map_domain` | `ds-cleaning::domain_plan` (`DomainPlan`, `cable_size`) |
| `table.filter` | `filter_sort_indices` with `FilterOp` (`equals`, `in`, `between`, `token_any`, …) |
| `table.sort`, `table.limit`, `table.sample` | `filter_sort_indices` / new seeded sample |
| `table.dedupe` | exact-geometry identity (today TypeScript `rawGeometryIdentity`; moves to Rust) or key columns |
| `table.aggregate` | new — group-by with `count`, `sum`, `min`, `max`, `mean` |
| `table.join` | new — key join (inner/left), with a declared cardinality check |
| `table.append` | layer merge (today TypeScript `data-factory-layer-merge.ts`; moves to Rust) |

#### Geometry ops — from `ds-geo-lite::operations` and the vector commands

| Op | Existing engine |
| --- | --- |
| `data.vector.measure` | `ds data vector measure` / `measurement` |
| `data.vector.buffer` | `operations::buffer` |
| `data.vector.sample` | `operations::points_along` |
| `data.vector.intersect` | `operations::intersections` |
| `geo.split`, `geo.trim`, `geo.extend`, `geo.offset`, `geo.project_onto` | `operations::{split,trim,extend,offset,project_onto}` |
| `geo.corridor` | `corridor`, `line_corridor` |
| `geo.filter_spatial` | `spatial_predicate` (intersects / within / contains) |
| `geo.overlay` (`clip`, `difference`, `union`) | `overlay`, `region` |
| `geo.extent` | `extent`, `oriented_extent` |
| `geo.properties` | `calculate_geometry_properties` (length, area, x/y) |
| `geo.admit` | `admission` — validate, repair what the policy allows, count the rest |
| `geo.reproject` | `ds-geo::projection` (explicit target CRS) |
| `geo.join_spatial` | new — attach attributes of the frame a feature intersects / is nearest to |

#### Enrichment — existing commands as ops

| Op | Placement |
| --- | --- |
| `data.admin-bounds.attach` | portable |
| `data.elevation.attach` | desktop |
| `data.parcels.query`, `data.customers.query` | cloud |

#### Sinks

| Sink | Effect |
| --- | --- |
| `file` (`geojson`, `geoparquet`, `csv`, `xlsx`) | `local_file_write`; never overwrites without `overwrite: true` |
| `map.local` | session-only local layer (`ds map local register`) |
| `design.stage` | staged design features (`ds map design create`); the design-intake path |
| `assets.ingest` | project asset; needs the project authority and `--yes` |

## 6. Lifecycle and modes

```text
draft ──validate──▶ valid ──plan──▶ plan.json ──run──▶ receipt
  ▲                                   │
  └──── edit (step add/move/remove) ◀─┘          save ──▶ revision@digest
```

| Mode | Reads data | Effects | Produces |
| --- | --- | --- | --- |
| `validate` | no | none | DAG, op resolution, parameter and schema errors |
| `preview` | sample (`--sample N`, seeded) | none | per-step schema, counts and the first rows of each step |
| `plan` | input metadata and pins only | none | `plan.json`: resolved ops and versions, input digests, schemas, estimates, authority, cloud cost, memory class, plan hash |
| `run` | full | sinks only | outputs plus receipt |

- **Plan then run, like `ds data spatial`.** `run --plan` re-checks the plan
  hash and input pins; a changed source refuses (`workflow_source_changed`)
  instead of running against different data.
- **Preview is the Data Factory loop.** It is cheap, never writes, and is what a
  person or agent iterates on before committing to a full run.
- **Memory class.** The plan states whether the run streams (all ops
  `row_local`) or needs whole frames, with the estimated peak. A run that would
  exceed the host's admitted budget refuses before it starts; it never relies
  on the OOM killer.

## 7. Arguments and expressions

There is no free-text expression language in v1. Conditions and computed
values use the structured shapes the engines already accept:

- **Predicates:** `{ "column", "op", "value" }` with `FilterOp`, combined with
  `{ "all": [...] }`, `{ "any": [...] }`, `{ "not": ... }`.
- **Computed columns:** the `CalculateOperation` variants.
- **Value maps:** `ds-cleaning` value replacements and domain plans.

A formula grammar is an open question (§14), not an assumption.

## 8. Authority, effects and confirmation

- A workflow's **effect** is the strongest effect of its sinks; its
  **authority** is the union of its sources', ops' and sinks' authorities.
  `ds workflow plan` states both before anything runs.
- A sink that writes project state (`design.stage`, `assets.ingest`) requires
  `--yes` on `run`, exactly as the standalone command would.
- `--project` is explicit on every project-touching plan and run
  (the Server never has an active project).
- Ops with placement `desktop` refuse on a host without a paired Desktop, with
  the same refusal as the standalone command. `cloud` ops carry the billing
  guard of `ds data spatial`.

## 9. Saved workflows, revisions and scope

- A saved workflow is an immutable **revision**, addressed as
  `<name>@<digest12>`; `<name>` alone resolves to the newest revision in scope.
- A revision records author, time, parent revision and a change note.
- Running a revision pins it in the receipt; editing creates a new revision and
  never mutates the old one.
- **Scope** (open question §14): project-owned for v1, with personal and
  governed global libraries considered later.
- Data Factory's **temporary** workflows are drafts in the local session
  store; they expire with the session unless saved. They follow the session-only
  rules of `ds-local-layers.md`.

## 10. The run receipt

```json
{
  "schema": "ds.workflow.receipt/v1",
  "run_id": "wr_…",
  "workflow": { "name": "pole-setback-parcels", "digest": "sha256:…", "revision": "pole-setback-parcels@3f9c1a2b7e01" },
  "params": { "radius_m": 30, "transformer": "beinamar_02" },
  "inputs": { "poles": { "digest": "sha256:…", "rows": 1240 } },
  "steps": [
    { "id": "new_poles", "op": "table.filter@1", "rows_in": 1240, "rows_out": 212, "ms": 4 },
    { "id": "zone", "op": "data.vector.buffer@2", "rows_in": 212, "rows_out": 212,
      "skipped": { "non_eligible_geometry": 0 }, "ms": 31 }
  ],
  "outputs": { "counts": { "path": "./setback-parcels.xlsx", "digest": "sha256:…", "rows": 7 } },
  "engines": { "ds": "…", "ds-geo-lite": "…", "ds-cleaning": "…" },
  "host": "server"
}
```

Determinism: the same revision, parameters and input digests produce the same
output digests on every host, except for ops declared non-deterministic
(none in v1; `table.sample` takes a seed).

## 11. CLI and MCP surface

All under one family, `ds workflow`, exposed as one MCP chapter with the usual
`describe` / `invoke` pattern. Every command is headless.

| Command | Effect | Purpose |
| --- | --- | --- |
| `ds workflow ops [--search <words>]` | read | List ops with placement, frames and schema transfer. |
| `ds workflow op <op-id>` | read | Full op contract (same as `ds capabilities <op-id>`). |
| `ds workflow new --out <file> [--from <revision>]` | local write | Start a draft. |
| `ds workflow step add\|set\|move\|remove --file <draft> …` | local write | Edit a draft one step at a time (the agent's recipe editor). |
| `ds workflow validate --file <draft>` | read | Structural, op and schema validation. |
| `ds workflow profile --file <draft> --input <name>` | read | Column profile: types, nulls, distinct values, geometry classes, extent. |
| `ds workflow preview --file <draft> [--sample N] [--step <id>]` | read | Sampled run with per-step rows. |
| `ds workflow plan --file <draft> --out <plan.json> [--param k=v] [--project <id>]` | local write | Resolve, pin and hash. |
| `ds workflow run --plan <plan.json> [--yes]` | per sinks | Execute; writes the receipt. |
| `ds workflow save --file <draft> --project <id> --name <name> [--note <text>]` | project write | Store an immutable revision. |
| `ds workflow list\|show\|history --project <id> [--name <name>]` | read | Browse revisions. |
| `ds workflow receipts --project <id> [--name <name>]` | read | Past runs of saved workflows. |

Existing single commands (`ds data vector buffer`, …) remain and are documented
as one-step workflows. Their help gains one line naming the op id.

**Refusals** (stable codes, each with a remedy):
`workflow_document_invalid`, `workflow_cycle`, `workflow_reference_unknown`,
`workflow_op_unknown`, `workflow_op_version_unsupported`,
`workflow_param_missing`, `workflow_param_invalid`, `workflow_schema_mismatch`,
`workflow_crs_required`, `workflow_host_incapable`, `workflow_authority_missing`,
`workflow_effect_unconfirmed`, `workflow_plan_changed`,
`workflow_source_changed`, `workflow_budget_exceeded`, `workflow_sink_exists`,
plus every refusal the underlying op already declares, passed through with the
step id.

## 12. Placement across repositories

Per the kernel/network boundary ruling (`ds-kernel-network-boundary.md`):

| Repository | Owns |
| --- | --- |
| `ds-command-kernel` | The workflow document, validation, DAG, schema propagation, plan/hash, receipts, revisions, the op registry, and every `portable` op (via `ds-geo-lite`). Must work with `ds-network` absent. |
| `ds-network` | `engine` ops: `ds-cleaning`, `ds-columnar`, `ds-geo` table ops and anything heavy. Registered into the registry by the host, never linked by the kernel. |
| `ds-cli` | The `ds workflow` family, MCP chapter, discovery prose and gates, and the skill. Thin: no op logic. |
| `ds-web` | Data Factory as a workflow builder, emitting steps and rendering previews; no transformation code. |

`ds-web` TypeScript that implements transformation today — the layer merge
(`data-factory-layer-merge.ts`), geometry merge (`data-factory-geometry-merge.ts`),
the table-tool wrappers (`attributeTable/table-tools.ts`) and the columnar
request glue — is deleted once the equivalent ops and the builder exist.

## 13. Acceptance examples

The contract is proven when these run end to end through installed `ds`, with
receipts, and with no Python or TypeScript in the path:

1. **Setback count** — the §3 document: buffer new poles, query parcels, count
   per village, write XLSX and a local layer.
2. **Survey CSV to staged LV design** — read a CSV with detected coordinates,
   rename and drop columns, map cable sizes through the domain plan, dedupe by
   geometry, stage as LV design features with `--yes`.
3. **Corridor crossing inventory** — sample an MV alignment every 10 m, find
   crossings with roads from the project cache, attach admin bounds, write
   GeoJSON.
4. **Data Factory parity** — a cleaning session performed in Data Factory
   produces a temporary workflow whose `ds workflow run` output is
   byte-identical to the session's applied result.
5. **Negative controls** — a step naming a missing column refuses at `plan`
   with `workflow_schema_mismatch`; a changed source between plan and run
   refuses with `workflow_source_changed`; a `desktop` op on a headless Server
   refuses with `workflow_host_incapable`.

## 14. Open owner questions

1. **Name.** "Workflow" for the document and `ds workflow` for the family; Data
   Factory keeps its name as the builder UI. Confirm.
2. **Scope of saved workflows.** Project-only in v1, or also personal and a
   governed global library from the start?
3. **Expressions.** Structured predicates and calculate operations only (this
   spec), or a closed formula grammar later?
4. **Timing.** SRE mode runs to the new year. Is any slice admitted earlier
   (for example, phase 1 validation/preview only), or does all implementation
   wait?
5. **Cloud execution.** Is a server-side run of a saved workflow (cloud
   placement for a whole run) in scope at all, or are workflows edge/Server
   only, with cloud reads as inputs?
