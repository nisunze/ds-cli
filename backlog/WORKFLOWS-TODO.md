# Workflows TODO

Implementation order for [DS Workflows](../docs/contracts/ds-workflows.md). Owner
direction 2026-10-07: workflows become the preferred data and geographic
processing interface; Data Factory becomes one way to build a workflow; agents
compose workflows through `ds`/MCP instead of patching data with geopandas,
pandas or Polars. **Specification only — nothing here is admitted for coding
until the owner answers the timing question (spec §14.4).** SRE mode runs to the
new year.

Each phase is one small, coherent slice: prove it through `ds` or the owning
tests, commit and push `run`, delete its line here. A phase that cannot be
proven stays here with the exact unmet condition.

## Owner questions first (spec §14)

- [ ] Name: `ds workflow` family, Data Factory kept as the builder UI.
- [ ] Scope of saved workflows: project-only in v1, or personal/global too.
- [ ] Expressions: structured predicates only, or a formula grammar later.
- [ ] Timing: whether any phase is admitted before the new year.
- [ ] Cloud execution of whole runs: in or out.

Move each into [OPEN-X.md](OPEN-X.md) if it is still open when phase 1 is
admitted.

## Phase 0 — Inventory (read-only, no code)

- [ ] List every transformation the Data Factory, design-upload wizard and
  attribute table perform today, with the file that does it (TypeScript or
  Rust/WASM). Start from `ds-web/src/lib/design/data-factory-*.ts`,
  `ds-web/src/lib/attributeTable/table-tools.ts`,
  `ds-web/src/lib/data-factory/`, `ds-web/src/lib/design/upload-*.ts` and
  `ds-network/docs/contracts/upload-cleaning-contract.md`.
- [ ] Map each to a spec §5.2 op id, mark it `portable` or `engine`, and
  record the gaps (ops marked "new" plus anything the inventory finds).
- [ ] Confirm the frame types against `ds-geo::table::TableDataType` and
  `ds-network/docs/contracts/columnar-format-contract.md`; record any
  mismatch as a spec amendment, not a third type system.
- **Proof:** the gap table is added to the spec as §5.3 and every Data Factory
  behaviour has a row.

## Phase 1 — Kernel document, validation and registry (`ds-command-kernel`)

- [ ] `ds.workflow/v1` types, canonical JSON and digest.
- [ ] DAG validation: references, cycles, unused steps, parameter binding.
- [ ] Op registry with the descriptor additions (`frames_in`, `schema_out`,
  `placement`, `row_local`); hosts register `engine` ops, the kernel never
  links `ds-network`.
- [ ] Static schema propagation and `workflow_schema_mismatch`.
- **Proof:** kernel unit tests over the four spec §13 documents (valid) and
  their negative controls (cycle, missing column, unknown op, bad parameter);
  the kernel builds and tests with `ds-network` absent.

## Phase 2 — Portable ops and frames (`ds-command-kernel`)

- [ ] Frame over Arrow batches with a WKB geometry column, admission at every
  source.
- [ ] Register `data.vector.{measure,buffer,sample,intersect}` and the
  `geo.*` ops from `ds-geo-lite::operations`, calling the same functions the
  standalone commands call.
- [ ] Move exact-geometry dedupe and layer append from TypeScript into Rust
  ops.
- **Proof:** for each vector op, a one-step workflow and the standalone
  command produce byte-identical GeoJSON on the same input.

## Phase 3 — CLI and MCP (`ds-cli`)

- [ ] `ds workflow ops|op|new|step|validate|preview|plan|run` (spec §11), thin
  over the kernel.
- [ ] One MCP chapter with describe/invoke; discovery prose that passes the
  existing CLI discovery gates.
- [ ] Standalone vector commands' help names their op id.
- **Proof:** spec §13 example 1 and 3 run through installed `ds` with receipts;
  the three negative controls refuse with their stable codes; a blind agent
  finds `ds workflow` from its own words.

## Phase 4 — Engine ops (`ds-network`, registered by hosts)

- [ ] Register `table.*` ops over `ds-geo::table` and `ds-cleaning` (select,
  drop, rename, cast, add_column, calculate, replace, clean_values, map_domain,
  filter, sort, limit, sample).
- [ ] `table.aggregate`, `table.join`, `geo.join_spatial` only when a real
  workflow needs them (YAGNI).
- [ ] Streaming for `row_local` ops via `ds-columnar` chunks; memory class in
  the plan; `workflow_budget_exceeded` before start.
- **Proof:** spec §13 example 2 runs headless on the Server to `design.stage`
  with `--yes`; a run that exceeds the admitted budget refuses before reading.

## Phase 5 — Saved revisions and receipts

- [ ] Immutable revisions `<name>@<digest12>` in the scope the owner chose;
  `save`, `list`, `show`, `history`, `receipts`.
- [ ] Plan-then-run guard: `workflow_plan_changed`, `workflow_source_changed`.
- **Proof:** save, edit, save again, run both revisions, compare receipts;
  mutate an input between plan and run and observe the refusal.

## Phase 6 — Data Factory as a workflow builder (`ds-web`)

- [ ] Every Data Factory action appends a step to a temporary draft; preview
  calls `preview`; apply calls `run`; "save as workflow" calls `save`.
- [ ] Delete the TypeScript transformation code: `data-factory-layer-merge.ts`,
  `data-factory-geometry-merge.ts`, the transformation parts of
  `attributeTable/table-tools.ts`, `data-factory/columnar-request.ts`, and
  their tests that pin TypeScript behaviour.
- **Proof:** spec §13 example 4 — the session's applied result and
  `ds workflow run` of its exported draft are byte-identical; the deleted
  files are gone and `npm run check` passes.

## Phase 7 — Design intake as a built-in workflow

- [ ] The design-upload wizard's mapping and column plans become a governed
  built-in workflow template ending in `design.stage`.
- **Proof:** a real project's LV intake runs headless through the template and
  matches the wizard's staged result.

## Phase 8 — Skill

- [ ] A `ds-workflows` skill in the shipped skill bundle: how to profile,
  compose step by step, preview, plan, run and save; the §13 examples as
  worked cases; "never reach for Python" stated once.
- **Proof:** a blind low-powered agent completes example 1 and example 3 from
  the skill alone.
