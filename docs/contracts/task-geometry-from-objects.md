# Task Geometry from DS Objects Contract

**Owner:** ds-command-kernel (the reference grammar, the resolution and the shaping are kernel decisions). Storage authority: ds-brain, unchanged — the task's top-level `geometry` and its `ds_object` links already exist in [work-links-and-spatial-anchors.md](../../../ds-brain/docs/contracts/work-links-and-spatial-anchors.md). Command surface: ds-command-kernel descriptors reached **CLI first** (`ds pm task …`), then MCP through the same descriptors; the desktop and the web render the same task later.

**Status:** requested by the owner on 2026-09-20; **lands in the current delivery, CLI first**. This is a feature request written as a contract. It adds to [project-management.md](../../../ds-brain/docs/contracts/project-management.md) and [work-links-and-spatial-anchors.md](../../../ds-brain/docs/contracts/work-links-and-spatial-anchors.md); it changes neither's authority and needs no ds-brain change.

**Related:** [ds-kernel-network-boundary.md](ds-kernel-network-boundary.md) (why the kernel never opens a `.dsgrid`), [ds-portable-geometry.md](ds-portable-geometry.md), [project-dsgrid-models.md](../../../ds-brain/docs/contracts/project-dsgrid-models.md) (model identity), [project-work-ds-objects-and-templates.md](../../../ds-brain/docs/contracts/project-work-ds-objects-and-templates.md) (typed `ds_object` subjects).

## Why

A consultant's review comment on the Nyamagabe MV design (18 Sep 2026) says that structures 74, 76 and 77 stand in a swamp. The task that answers it (T4) has to say WHERE: on the map and in the plan, the work is an area around three named structures, not a title. Today a task can carry one top-level geometry and typed `ds_object` links, but only the map's drawing tools author the geometry, and nothing turns "structures 74, 76, 77 of this model" into it. An agent reading the comment can supply the numbers it read; it must not draw.

The owner's ruling is propose → confirm with no LLM in the stack: the intelligence (a person, or an agent reading a record) supplies the **typed object references** it read; the kernel resolves them **deterministically** to geometry; the CLI shows the proposal (`--dry-run`); a person confirms with `--yes`. The CLI never parses prose.

## Definitions

| Term | Meaning in this contract |
|---|---|
| **Reference** | One typed string naming DS objects: `dsgrid:<source>:structure:<sel>` or `dsgrid:<source>:alignment:<aln>[:<a>..<b>]`. The grammar is closed (§Reference grammar) and parsed by the kernel. |
| **Source** | Where the objects' model comes from: `local-<id>` — a DS Grid working copy in this machine's catalogue (`ds dsgrid model list`, scoped by the session's lane and account); or `package` — the `.dsgrid` file named by `--package <path>`. |
| **Object index** | The deterministic engine result a host produces from one `.dsgrid`: every structure's stable id, engineering number, alignment and station, and every alignment's polyline, **all in EPSG:4326**, plus the model's identity (`model_id`, `model_revision`, `snapshot_fingerprint`, declared CRS). The kernel never reads a package ([ds-kernel-network-boundary.md](ds-kernel-network-boundary.md) §1); it reads this index. |
| **Resolution** | Matching each selector of a reference to exactly one object of the index. Deterministic; unresolved and ambiguous selectors are named, never guessed. |
| **Shaping** | Turning the resolved objects into the task's ONE top-level geometry by a declared rule (§Shaping rules). |
| **Proposal** | The receipt of `--dry-run`: the geometry, the links, the rule applied, the objects and their sources. Nothing is written. The same bytes are written by `--yes`. |

## Authority and invariants

- **The kernel decides; hosts perform effects.** The grammar, the resolution, the shaping, the bounds and every refusal are `ds_command_kernel::task_geometry`, pure and portable. Reading a `.dsgrid` is heavy engine work (`ds-grid-exchange`, `ds-geo`) and stays in a host that legitimately links `ds-network`: today `ds-cli-dsgrid` builds the object index; the desktop can build the same index from its open session later. There is no Cargo edge from the kernel to ds-network.
- **Reprojection is the existing one.** The index builder reprojects with `ds_geo::projection::GridModelCrs::to_map`, exactly as `ds-project-data::mv_projection` reprojects the MV plan for printing. No new projection code; a model whose declared CRS that vocabulary does not know is refused `model_crs_unsupported`.
- **One geometry, one rule.** A task owns one top-level WGS84 geometry (Point | LineString | Polygon, ≤ 2,000 positions, closed ring, no holes). A proposal applies exactly one shaping rule and names it in the receipt.
- **The link is the trace.** Every resolved object becomes one `kind: ds_object` link on the task, so the plan can say which structures the area came from and the object stays traceable when the geometry is later redrawn. Targets never grow backlinks; a `.dsgrid` is never written.
- **Everything through ds-brain.** The write is the ordinary `commit_command` (`create_task` with `geometry` + `links`, or `update_task_fields` with `geometry` + `links`) against the current graph revision; `work_revision_conflict` on a moved graph; one revision per command. No new ds-brain action, field or index.
- **DS Grid objects only, by contract.** A survey entry is linked (`survey_entry`) and its geometry is never copied into the task ([work-links-and-spatial-anchors.md](../../../ds-brain/docs/contracts/work-links-and-spatial-anchors.md) — "it is not copied into this top-level field merely to make the link work"); a transformer is a collection of objects with no single canonical position and is linked as a typed subject through the existing work-template flow. A structure in a `.dsgrid` is the one object kind whose position exists nowhere ds-brain can resolve — the model is a package — so the task carries the geometry it derived and the link names the object it derived it from. `transformer:` and `survey:` references are therefore **not** in the grammar; a reference with any other head is `reference_invalid`.
- **Confirmation.** The commands are `global_write`. `--dry-run` is the declared previewing path of a writing command: with it set nothing is written and no `--yes` is required — the ds-cli contract crate gains exactly that rule (`Command::confirmation_required_for`), beside the existing `--write` trigger. Without `--dry-run`, `--yes` is required as for every other plan write.

## Reference grammar

```
reference  := "dsgrid:" source ":" object
source     := "local-" id            # this machine's DS Grid working copy (ds dsgrid model list)
            | "package"              # the .dsgrid named by --package <path>
object     := "structure:" sel ("," sel)*
            | "alignment:" aln
            | "alignment:" aln ":" sel ".." sel
sel        := <engineering number>   # the structure number a consultant writes: 74
            | <structure id>         # the model's stable StructureId
aln        := <alignment id> | <alignment label>
```

- Selectors are trimmed; an empty selector, an empty list, a repeated selector, a source that is neither `local-<id>` nor `package`, or a head other than `dsgrid` is `reference_invalid` naming the reference and the part.
- A selector resolves first against `engineering_number` (exact after trimming), then against the structure's stable id. A number that names two structures (two alignments numbered from 1) is `object_ambiguous` naming the candidates with their ids and alignments; the remedy is the id.
- `aln` resolves first against the alignment id, then its label; two alignments with one label is `object_ambiguous`.
- `<a>..<b>` names the two bounding structures ON that alignment (number or id; a structure not placed on it is `object_unresolved`); the order is by station, whichever way it was written.
- A `local-<id>` that the catalogue does not hold is `model_unknown` naming the id; a `package` reference with no `--package` is `model_unknown` naming `package`. A path that is not a readable `.dsgrid` keeps the `ds dsgrid` refusals (`model_not_found`, `not_a_dsgrid_package`, `package_decode_failed`).
- Several references may be given (`--from` repeats, `--geometry-from` repeats); they may name different sources. All of them are resolved before anything is shaped.

## Shaping rules

The rule is chosen by what was resolved, and the receipt names it under `rule.kind`.

| Resolved | Rule | Geometry |
|---|---|---|
| exactly one structure | `point` | `Point` at the structure |
| two or more structures, no alignment | `buffered_hull` | `Polygon`: the convex hull of the structures, buffered by `buffer_m` (default **25**, admitted **1 … 500**; outside → `buffer_out_of_range`), as the convex hull of a 16-gon of radius `buffer_m` around each hull vertex, computed in a local metric frame (equirectangular about the centroid, WGS84 radius) and returned in EPSG:4326; a closed ring, no holes |
| one whole alignment | `alignment_line` | `LineString` of the alignment's polyline |
| one alignment range `<a>..<b>` | `alignment_range` | `LineString` of the polyline between the two structures' stations, both ends exactly at the structures |
| anything else (an alignment beside structures, two alignments) | — | `references_incompatible` |

- Two structures give a capsule, three a rounded triangle: a "swamp crossing at 74/76/77" becomes a visible area, which is the point.
- Positions are bounded at 2,000 (`geometry_too_large`, naming the count); objects are bounded at **100** per proposal (`too_many_objects`, naming the count and the bound), because a task may carry at most 128 links and the objects become links.
- Coordinates are rounded to 7 decimals (≈ 1 cm); the receipt carries `rule.positions`.

## Links

One link per resolved object, in resolution order, appended to the task's existing links (a link with the same `kind + project + object_type + entity_id` is the same link; ds-brain keeps its first attachment):

```json
{"kind":"ds_object","project_id":"<project>","object_type":"dsgrid_structure",
 "entity_id":"<manifest model_id>:<structure id>",
 "object_revision":"dsgrid:r<model_revision>:<snapshot_fingerprint>",
 "label":"Structure 74"}
```

- `object_type` is `dsgrid_structure` or `dsgrid_alignment`; `entity_id` is `<manifest model_id>:<entity id>` (the native manifest identity the project catalogue pins as `manifest_model_id`, so a working copy and its published head name the same object); `object_revision` is the observation — the package revision and fingerprint the geometry was read from; `label` is `Structure <number>` (the id when unnumbered) or the alignment's label. An `entity_id` past ds-brain's 256-byte bound is `reference_invalid`.
- A range links the alignment and every structure of the range (bounded by `too_many_objects`).
- ds-brain reconciles these subjects as `unsupported` today (only `transformer` has a resolver); that is the read-only projection the contract already defines, not a refusal, and it changes when a DS Grid resolver is deliberately added.
- Existing links plus new links past 128 is `links_bound_exceeded`.

## Command surface (kernel descriptors; ds-cli first)

Server-native (`Requires::Server`, `Authority::HeadlessProject`, `--lane`), through the same headless PM door `ds pm plan` uses. Effects: `global_write` unless stated.

| Command | Inputs | Notes |
|---|---|---|
| `pm.task.create` | existing + `--geometry-from <ref>`* `[--package <path>] [--buffer-m N] [--dry-run]` | native on this delivery (`commit_command` / `create_task`); with `--geometry-from` the task is created carrying `geometry` + one `ds_object` link per object, in ONE revision; `--dry-run` answers the proposal and creates nothing |
| `pm.task.geometry.set` | `--task <id> --from <ref>* [--package <path>] [--buffer-m N] [--dry-run] --yes` | `update_task_fields` with `geometry` and the merged `links`, in ONE revision; an existing task made any other way (`--from-record`, the map, the Plan sheet) takes its geometry here |
| `pm.task.geometry.clear` | `--task <id> --yes` | `clear_geometry` and drops the task's `dsgrid_structure` / `dsgrid_alignment` links (the trace of a geometry that no longer exists); every other link stays |
| `pm.task.geometry.read` | `--task <id>` | `read_only`; the task's `geometry`, its links, and ds-brain's reconciled `subject_states` for them — the read `ds pm plan` and the map's tasks layer paint from |

Receipts: `create` and `set` answer `project`, `taskId`, `committedRevision`, `warnings`, `link` and the same `proposal` a dry run answers (`geometry`, `rule`, `objects`, `links`, `sources`); `--dry-run` answers `dry_run: true`, `proposal` and `committedRevision: null`.

## Kernel operation — `ds.task-geometry/v1`

`ds_command_kernel::task_geometry::evaluate(bytes)`, one request per call:

| Action | Request | Reply |
|---|---|---|
| `parse` | `{schema, action, references: [string]}` | `{references: [{reference, source: {kind: local\|package, id?}, object: {kind: structure\|alignment\|alignment_range, selectors[] \| alignment, from, to}}], sources: [distinct sources]}` — what a host must load before proposing |
| `propose` | `{schema, action, project, references: [string], buffer_m?, existing_links?: [link], index: {sources: [ObjectIndex]}}` | `{geometry, rule: {kind, buffer_m?, positions, objects}, objects: [{kind, id, number?, label, model_id, model_revision, position \| positions, alignment?, station_m?}], links: [new links], sources: [{ref, model_id, model_revision, fingerprint, crs}]}` |

`ObjectIndex`:

```json
{"ref":"local-abc","model_id":"m-…","model_revision":3,"fingerprint":"sha256…","crs":"EPSG:32735","display_name":"Nyamagabe MV",
 "structures":[{"id":"s-74","number":"74","alignment":"aln-1","station_m":1830.5,"position":[29.6112345,-2.3412345]}],
 "alignments":[{"id":"aln-1","label":"Line 1","vertices":[[29.61,-2.34,0.0],[29.62,-2.35,1412.7]]}]}
```

Each vertex carries its station along the alignment (metres, from the geometric chainage in the model CRS) so a range is clipped by station in the kernel without metric arithmetic on lon/lat. An index whose `ref` no reference names is ignored; a reference whose source the index does not carry is `model_unknown`.

Refusals (kernel, adopted by the CLI under the same names): `reference_invalid`, `model_unknown`, `object_unresolved` (names every unresolved selector, per reference), `object_ambiguous` (names the candidates), `references_incompatible`, `too_many_objects`, `geometry_too_large`, `buffer_out_of_range`, `links_bound_exceeded`, `model_crs_unsupported` (host, at index time).

## Discoverability and parity

- `ds capabilities --search "task geometry"`, `"where is the task"`, `"attach structures to task"`, `"polygon"`, `"swamp"` find the family; help stays inside the context budget; `docs/reference/pm.md` and `docs/reference/dsgrid.md` cross-reference.
- MCP reaches the same descriptors: an agent that reads a record's text supplies the structure numbers it read and calls `pm.task.create --geometry-from … --dry-run`, then the same with `--yes` once the person confirms. That is the whole "semantic" step, and it is governed by `--yes`.
- No `CLIENT_PROFILE_SCHEMA` bump (the PM door is the one `ds pm plan` already uses); the WINDOW ratchet of `ds-cli-pm` falls by one (`pm.task.create` leaves the paired route), never rises.
- The skill `ds-task-geometry` says: a comment names structures → `ds pm task create --title … --geometry-from dsgrid:local-<id>:structure:74,76,77 --dry-run`, read the proposal, re-run with `--yes`.

## Acceptance — T4 from the CLI

"Landed" means the following runs green against a TESTING project (`agct38sq_sample`) on lane canary with the headless credential, from a synthetic working copy (never the TBEA workspace):

```bash
ds dsgrid model create-local --name "swamp-fixture" --crs EPSG:32735 --lane canary --account <uid>   # or import-external a synthetic package
ds pm task create --title "Swamp crossing at 74/76/77 — relocate or pile" --kind inbox \
   --geometry-from dsgrid:local-<id>:structure:74,76,77 --dry-run --lane canary --output json   # proposal: Polygon, 3 links, rule buffered_hull
ds pm task create --title "…" --kind inbox --geometry-from dsgrid:local-<id>:structure:74,76,77 --lane canary --yes --output json   # taskId, revision
ds pm task geometry read --task <id> --lane canary --output json                                   # geometry + 3 ds_object links
ds pm task geometry set --task <id> --from dsgrid:local-<id>:alignment:aln-1:74..77 --dry-run --lane canary --output json   # LineString
ds pm task geometry clear --task <id> --lane canary --yes --output json
```

## Out of scope for this delivery

- No governed project model source (`dsgrid:project:<model-id>@<rev>`): download the head with `ds dsgrid project download` and pass it as `--package`. Recorded as the next source when a task must cite a published revision by catalogue identity.
- No `transformer:` / `survey:` references (by contract, above), no free-text parsing, no OCR, no LLM in the stack.
- No desktop or web authoring dialog; both render what the task now holds through the existing tasks layer.
- No ds-brain DS Grid subject resolver; reconciliation reads `unsupported` for these links until one is added deliberately.

## Steer for implementation

ds-command-kernel (`task_geometry`: grammar, resolution, shaping, bounds, tests on a synthetic index) → ds-client-core (`project_management::Command::Commit`, the write door beside the read door) → ds-cli (`ds-cli-dsgrid::objects` index builder over the engine crates; `ds-cli-pm` geometry family + native `create`; contract crate `--dry-run` rule; reference docs; skill) → pins re-pointed → acceptance script green → merged to run.
