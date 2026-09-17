# Printing document authority

`printing::Layout` is a closed `ds.print-layout/v2` document. Page dimensions and
frames use millimetres; typography uses points. It persists paper size and
orientation and contains one named, clipped print viewport,
text/legend/scale/north/logo/rectangle elements, held PNG assets and keyed
physical cartographic styles. The generated JSON schema is `layout_schema()`.
The map element's rectangle is the physical viewport. Its `view_slot` is bound
to a project named geographic view when an export is prepared; editor pan/zoom
is not part of this document.

`elements` is the canonical back-to-front sheet stack. The final element is
front-most. `bring_to_front`, `send_to_back`, `move_above` and `move_below`
change only this order. Map-layer ordering inside the print viewport is a
separate reporter input.

`printing::evaluate` evaluates New, Validate, Edit (upsert one element),
Remove, SetPaper, SetViewport and the four stack-order commands. Every result
contains `layout`; validation additionally returns
`valid:true`. Invalid edits produce no partial document. CLI and the web WASM
export `printingCommand` call the same evaluator. `command_schema()` describes
the closed intent grammar. The native renderer can validate the same Layout.

Spatial hit testing, snapping and retained camera sessions remain in ds-network;
they are not command-kernel WASM exports. PDF/SVG formatting, GeoJSON projection
and legend rendering belong to ds-network-reporter. Brain owns global/project
sharing and expected-revision saves. This module owns no storage or network IO.

`new` accepts optional `id`, `name`, `paper` (A0–A5 or bounded custom paper,
default A3) and
`orientation` (portrait/landscape, default landscape). Paper presets uniformly
fit the initial composition. `rename` changes only the name; `duplicate`
requires a different ID and carries the composition into an independent layout.
CLI `report.layout.edit --request FILE` and web authoring use these same intents.

## Plain engineering schedule visibility

`table_columns` returns column choices from the selected table's source and
additional source layers. Populated zero and false values count as data; empty
values do not. Existing template columns remain visible in the choices when
one transformer has no values, so inspecting it does not silently remove
columns from the project template.

`table_column_selection` accepts the selected property keys and preserves
their authored headings and physical width constraints. New columns must have
data. At least one unique column must remain. `table_phase` authors `design`
or `as_built`; design removes installed meter/contact fields. Native validation
enforces the explicit phase too. An absent phase preserves legacy layout
compatibility. These intents use plain engineering layers rather than Excel
schedule rows. Network Summary retains `workbook_info` presentation.

Column visibility and print pen overrides belong to the saved template. Saving
a project template applies them to subsequent sheets using that template; it
does not change engineering data or silently regenerate existing printouts.

`ds.print-layout/v1` remains an accepted input only. Every evaluated V1 command
returns V2: standard paper is inferred from exact dimensions, other bounded
dimensions become custom paper, and the legacy map frame becomes the clipped
`primary_map` viewport. New output is never V1.

## MV delivery ownership

MV is the common delivery scope for DS Grid MV models and their attachments,
MV maps and printouts, other MV reports, the staking table, and the future
BOQ. Index and package these together through the MV reporting workflow.
Generated outputs retain the exact source model version and digest; an attached
submission file retains its immutable attachment revision. Grouping artifacts
must reference their existing storage, not copy them into another catalogue.
Printing or packaging does not create or advance a design model version.

LV also carries tags and attachments. Treat an attachment as a typed tag/link
to an existing blob reference, retaining its immutable digest, access rules,
and any exact design-version binding. MV and LV use the same reference
semantics; grouping or attaching a file must not duplicate its stored bytes.
An attachment reference remains distinguishable from a plain categorical tag.

Prove the existing headless CLI/MCP behavior before changing this workflow.
Where the proof exposes a missing operation, extend its owning contract in a
bounded slice and verify the result through CLI/MCP before adding UI behavior.
This scope does not authorize generating a future BOQ or changing quantities.

## Fixed legend anchors

`legend.align` chooses `left` or `right` within the authored legend frame.
The renderer measures the legend content and holds that edge as rows and
categories change. Omitted alignment preserves the historical `right` anchor.
Use `left` for a lower-left legend, remove its `flow`, and keep it out of
`composition.movable` to retain the authored position.

## What a sheet layout optimises

Sheet composition is an optimisation, not a set of placements. The objective is
to **maximise the usable map area**, subject to hard constraints that are never
traded against it:

- every table row and user-selected field present and readable at the authored type size;
- the 5 mm sheet margin;
- aligned edges;
- no overlap, no clipping.

Panel count and table shape are therefore **decision variables, not authored
constants**. A schedule may take more panels, or a different shape, when that
returns map area — and a layout that fixes them by hand has removed the
optimiser's only freedom. Type size and content are NOT decision variables:
shrinking text or dropping a column buys area by breaking a constraint, which
is not a solution.

Read the rest of this document as the constraint set. "It fits" is not the
standard; "it fits and nothing else would have given the map more room" is.

## Sheet margin

Tables, schedules and legends sit against the sheet border with ONE small
margin. The margin is 5 mm — the same constant as the measured content gap, so
the space between content and the page reads as the space between two content
frames.

A content frame must not float in unused space. The stack begins at the margin
and grows inward: a table anchored left starts one margin from the left border,
a table anchored right ends one margin from the right border, and the first
frame in a vertical stack starts one margin below the top border. Whichever
borders a frame is anchored to, it hugs them.

Unused frame width is not a position. An authored frame may be wider than its
measured content, and the content is placed against the border regardless —
the same rule `flow` already applies between frames, applied to the page edge.

This is the default. It is not a per-sheet decision to be made again on every
layout, and a sheet that floats its schedules away from the border is wrong
even when nothing overlaps.

`element.sheet_anchor` exposes `auto` (default), `top_left`, `top_right`,
`bottom_left`, `bottom_right`, and `authored` through the layout schema and
per-transformer overrides. Auto uses the table/legend's horizontal alignment
and whether its authored origin is in the upper or lower half of the page. The measured content
hugs those edges at 5 mm; changing row counts never changes the chosen edge.
`flow` takes precedence and attaches to its measured predecessor. Explicit
`authored` retains free placement for a deliberate composition; it is not the
schedule default. Automatic composition also packs inside the same page inset.

## Schedule fields follow the workbook

A printed schedule carries the fields its workbook carries. The exported table
is the field list: bind the schedule to it rather than re-choosing a narrower
set per sheet, the way the information table already works. A sheet that prints
`House | Pole | Customer | Category | Svc m` when the workbook holds eleven
columns has dropped real content the reader needs.

The customer/house-connection schedule is:

| Column | Heading |
|---|---|
| pole | Pole Number |
| house | House Number |
| names | Names |
| meter_type | Meter Type |
| category | Category |
| from_tr | From Tr Distance |
| nid | Nid |
| service_area | Service Area Length M |
| village | Village |
| x | X |
| y | Y |

**As-built-only fields are omitted when the project is not as-built.** These
identify an installed, metered connection and carry no meaning for a design:

- `meter_type`
- `nid`
- any phone number

Everything else prints. Omission here is about meaning, not width: a field is
dropped because a design cannot have it, never because the sheet is crowded.
If the schedule does not fit, that is a placement, panel or font decision — see
`flow` and `overflow` — and never a reason to drop a meaningful column.

## Measured content flow

`flow.after` names an earlier table or legend. `placement` defaults to
`below_right`, preserving the existing vertical stack with a shared right edge.
`below_left` stacks at its measured left edge. `right_top` places the next
schedule beside its actual content width with a common top edge. `gap_mm: 0`
permits contiguous panels/schedules. These placements follow content bounds as
row and panel counts change; they never use unoccupied maximum frame width.
Use left-aligned table/legend bindings with the latter two placements.

## Local custom area preparation

The report command exposes `custom_area_plan` before host IO and `custom_area`
after exact national boundary capture. They share project/map identifier and
one-to-eight distinct sector-code validation. Capture requires exact sector
identities and levels, valid polygonal geometry, a connected selection, and
at most 50,000 coordinates across the result. It preserves holes and constituent
polygons; it does not claim to dissolve shared edges. The stable sorted capture
has a SHA-256 over its serialized MultiPolygon. Default projection contains
names, bounds and digest; complete coordinates require `include_geometry`.
This is area preparation, not a rendered report or an offline-readiness claim.
The host fetches through existing reference authority; it cannot replace missing
boundaries with a rectangle. Custom print output remains local until the project
asset lifecycle is implemented.

## Workbook-styled schedules

Bind `table.workbook: {"sheet":"customers","as_built":false}` (or `poles`,
`lv_lines`, `service_cables`) instead of `layer`, `columns`, `headings`, widths
and sort controls. The workbook owner supplies its actual ordered, relevant
fields, full headings, display values and pole line sections. Empty or uniform
fields follow the export's existing selection rules; design additionally omits
only the as-built fields listed above. Never remove a retained field for width.

`table.presentation` accepts `auto` (default: Excel for workbook bindings,
plain for generic bindings), `excel`, `plain`, and `workbook_info`. Excel uses
workbook header/title colours, alternating body bands, text left and numbers
right, with workbook numeric coercion and rounding before measurement.
Generic table bindings remain available. The limit is 26 columns (A–Z), raised
from 12 because the existing pole workbook can have 14; no workbook field is
lost to that old limit. Use panels, available paper area and type size to fit.

The historical Zihare PDF illustrates table presentation only. It does not
replace the project's composition or justify removing existing/proposed MV
context. Layout geometry, flow, anchors, style overrides and legend controls
remain public CLI/MCP inputs; composition requires no source-code inspection.

## Indexed table headings

`table.heading_mode` is `auto_index` (default), `literal`, or `indexed`.
`auto_index` chooses indices when any authored heading exceeds its data-sized,
minimum/maximum-constrained column, so **a heading never widens its column**: one
long name like "Service Area Length M" over short numbers no longer sets the
column width and pushes real content off the sheet. A table whose headings
already fit is unchanged, and `literal` remains available for a layout that
deliberately wants full headings in the row. Indexed tables use stable column-order
letters A–Z for every column and an untruncated explanatory key above each
panel. Full headings remain in the document; columns and values never change.
The full key repeats above each measured data panel, wrapping at the authored
type size within that panel width. It never extends into unused authored frame
width or creates a gap before an adjacent schedule. This corrects the former
full-frame key behavior, which visibly separated the Zihare schedule bodies.
The key consumes actual table height;
row omission warnings must use that same remaining height. `measure_headings`
continues to govern literal headings; indexed headings measure the letters.
Per-transformer `table.heading_mode` overrides the project layout through the
same existing print-instruction command. Old documents omit the default field
when serialized, preserving compatibility and their existing presentation.

## The shared preview page

A preview is one transformer rendered through the delivery engine with a
draft layout — any id, any paper, saved or not — as the engine's own page:
the SVG every delivered format is derived from (PDF by conversion, PNG and
JPEG by rasterisation), with text flattened to outlines through the same
bundled font, so a viewer needs no font and draws the sheet exactly as the
delivery comes out. One preview therefore serves every format a template may
later be delivered in, and every host paints the same page: the desktop over
its Canvas2D sheet, an agent through any SVG viewer.

`report_export::preview::with_layout(receipt, layout)` is the recipe both
hosts run. It rewrites the governed receipt in memory only: the draft
replaces (or joins) `printing_setups` under its own digest, the output policy
becomes exactly one `svg__<id>`, and `local_print_recipe.preview` names the
run. `publication_facts` reads that recipe as `local_only`; hosts never
place the page under an artifact root. `PrintArtifactFormat::Svg` is not
`deliverable()`: `normalize_output_selection`, a legacy token list and
`project_output_patch` refuse it, while `stored_output_selection` and
`selected_plan` read it, because the engine reads the in-memory policy through
them. The host save paths are the boundary; ds-brain admits only pdf, png and
jpeg in a stored selection.

`printing::preview::plan` answers whether the editor may preview right now,
from facts: the draft, the chosen transformer as `printable_transformers`
listed it, the governed print styles the host holds, online and native-engine
readiness. Refusals are keyed (`printing_preview_no_transformer`,
`printing_preview_not_a_transformer`, `printing_preview_room_unavailable`,
`printing_preview_native_unavailable`, `printing_style_unavailable`); a dirty
held room is a note, not a refusal. A room this device does not hold is
refused whether or not the host is online: a preview reads held state and
never downloads a room — holding one is the Design/Status page's download
action. The plan also chooses the raster the host draws the page at from the
screen's pixels per millimetre (`raster`): between 1.5 px/mm and the 300 dpi
delivery raster, within a 40-megapixel budget. Re-rasterising at a new zoom
never involves the engine.

`ds-report-host::preview::execute(engine, &PreviewSettings, &PreviewRequest,
&HeldState) -> Result<PreviewPage, PreviewRefusal>` is the one execution of
`report.project.export --preview-layout` both hosts reach. The command is
declared once, by its descriptor in ds-cli (`ds capabilities` and MCP
project it), and executed by exactly one door per host process: `ds`'s own
`registry::dispatch` and the desktop's Tauri command `ds_invoke`. Both doors
are adapters: they build the same `PreviewRequest {transformer, layout}` from
the same declared argument names (`transformer`, `preview-layout`), hand the
same kernel-typed held state, call the same `execute`, and wrap the same
`preview::answer(project, lane, &[(page, PageRef)])` data — or the same
`PreviewRefusal` — in the same DS envelope v1 from `ds_command_kernel::envelope`
(`{v, command, contract, status, data | error{class, code, message,
retryable, remedy?, next?, detail?}}`), under `preview::COMMAND_ID` and
`preview::CONTRACT`. `PreviewRefusal::class()` reads one table,
`preview::CLASS_TABLE`, keyed by code alone, so the two doors and ds-cli's own
refusal table classify identically; a kernel refusal's `{message_key, params}`
travels as `error.detail` for the localised surface.

Inside, `execute` composes `transformer_page` — which takes the governed
receipt, the held room, the draft layout and a print-context closure, upgrades
the draft, applies `with_layout`, decides the draft's context layers with the
kernel (`report::evaluate` `context_preparation`), stages and runs the engine
through the same `run_transformer_report_with` a delivery uses, proves the one
`svg__<id>` page (exactly one artifact, at most 96 MiB, its sha, UTF-8,
`<svg`) and returns page, filename, run receipt, `print_diagnostics`,
warnings and the context summary in memory, leaving nothing on disk — with
the one held-context resolver, `ds_project_data::held::preview_context`.

The held-state vocabulary is the kernel's own structs, `deny_unknown_fields`,
and every decision over it is a kernel call: `HeldState {room: HeldRoom,
context: HeldContext {captures: Vec<HeldCapture>, retained:
BTreeMap<String, RetainedContext>, holdings: Option<Holdings {root, scope,
catalog}>}}`. The room is admitted before `execute` by
`printing::preview::room_admission` over `RoomFacts` reduced from the row the
local design store keeps (dirty and incomplete → `print_preview_room_incomplete`,
absent, foreign or empty → `print_preview_room_unavailable`, the server
version read from it); the CLI's room is the service answer, admitted by its
own export path — both feed the same `HeldRoom`. The resolver then, for
exactly the draft's context layers: `printing::held_context::admit` admits a
survey or local-layer capture (`print_context_survey_empty`,
`print_context_too_large`, `print_context_capture_changed`,
`print_context_foreign_project`, `print_context_capture_incomplete`, at most
`MAX_HELD_FEATURES` = 50 000 features) — a refused capture ends the preview
with that refusal unchanged, a missing one is the omission "not captured on
this host"; `printing::context::retained_admission` reuses a retained
elevation-contour or project-MV context only under the retained schema, with
exactly this source's parameters (`retained_parameters`, the browser's
`JSON.stringify(source, sortedKeys)` spelling), covering the wanted extent
and naming only the layers its preparation emits (for the project MV
projection: lines, structures and angle points) — anything else, including
nothing retained, is a named omission on the sheet with the kernel's reason,
never an acquisition; catalogue, building and unretained contour layers are
read from the project's rooms under `holdings` in `Mode::Read`, which omits
what is not held, and without holdings are omitted as "no holdings root on
this host". Everything held and read merges through `attach_layers` into one
`ds.print-context/v1` document under one digest. A preview never downloads
a room and never acquires or projects context; a delivery or seed still
prepares context through its own path.

Two web limitations are declared, not designed around. A `<json-file>`
input is a document: the CLI names the draft by path and reads it, the
desktop door receives it inline because the webview has no filesystem. The
page is placed by the host that has a filesystem: the CLI writes
`<out-dir>/<transformer>/<page.filename>` and `report-run.json` from
`page.receipt` beside it and answers `page: {path}`; the desktop answers
`page: {svg}` inline (`PageRef::File` / `PageRef::Inline`). `out-dir`,
`lane`, `admin-bounds` and `concurrency` are host-owned on the desktop and
refused as arguments there. The `data` member is otherwise the same on both:
`results[]` rows with `transformer`, `server_version`, `status`, `page`,
`filename`, `sha256`, `size_bytes`, `content_type`, `engine`, `elapsed_ms`,
`warnings`, `print_diagnostics` and `print_context {sha256, layers,
omitted[{layer, reason}]}`, plus `preview {layout_id, layout_sha256,
output_id, format, note}` and a completed `batch` summary. Headlessly the
same command id produces the same page on a Server host through the same
staging and recipe; no `desktop.*` or `server.*` twin exists.

## Template pens

`Layout::style_overrides` are the template's pens: colour, size, opacity,
visibility and label treatment over the governed `_print` document, applied
at render (`style_overrides::apply`) and read by the legend, so a preview and
a delivery print the same pens. `printing::pens::pens(layout, documents)`
lists them for a host's pen panel — one row per bound print layer, design
layers first in engineering order, base values read from the resolved
document given as a fact, the effective values after the override, the
category table of a data-driven colour, the size unit from the document's
print scaling — and `pens::edit` sets one bounded property (`color`, `size`,
`opacity`, `visible`, `label_visible`, `label_size_pt`, `label_color`,
`label_halo_mm`, `label_halo_color`; `null` clears it, an empty override is
removed). A layer without a bound style refuses `printing_pen_unbound`.
`print_pens`, `pen_edit` and `set_style_override` are `printingCommand`
operations; `ds report layout pens` is the same surface headlessly.

## Printable inventory and preview admission

`printable_transformers` takes status `rows`, optional account-fenced host
`rooms`, `current_user`, and a `limit` of 1 through 500. Reserved names use
`design::row_kind`, so aggregate, analysis and project-document rows remain
excluded even when native status omits `kind`. Supplied room facts produce
`cached` / `dirty` booleans; omitted room facts produce nulls and
`local_rooms_known: false`. An empty supplied inventory is known empty.

`render_plan` validates the layout, requires all referenced print styles and
some map data, and returns `ready`, refusal keys, required style references
and text with the host-supplied date. Rendering remains a host effect.

Report `export` requires an overview's `active_project` fact to match
`project`; hosts re-evaluate after asynchronous preparation. Individual
transformer exports need no open project. `batch_outcome` orders completed
results and counts errors without performing any export. These decisions are
available natively through `ds report transformers`, `ds report plan`, and
`ds report layout edit` as well as WASM.

## The print-context document

`report_export::print_context_document(coverage, sources, layers)` is the one
recipe for the `ds.print-context/v1` asset a host stages beside a transformer's
room and the engine (`ds-network-reporter` `load_print_context`) reads
digest-bound. The desktop shell assembled that document itself until now; with
one kernel recipe a desktop and a headless server given the same coverage,
sources and layers stage byte-identical assets under one digest
(`report_export::sha256_hex`).

The document is `{"schema","coverage","sources","layers"}` in that order,
layers in the map's own order, compact serialization. Validation mirrors what
the engine would refuse: `coverage` is a Polygon or MultiPolygon; every layer
id is an identifier and its collection exactly `{type: FeatureCollection,
features}` of Features with object properties; no layer is empty (the engine
drops an empty collection silently — a host names what it could not supply in
the run receipt instead); every `sources` entry names a layer of the document;
at most 64 layers, 100 000 features and 64 MiB.

A run receipt (`report_export::run_receipt`) records `print_context_sha256`
(null when the run printed without external context), `print_context_layers`
and `print_context_omitted`, so a receipt never reads as complete when its map
context was not.

## Surface optimization

Furniture is a constrained packing problem. For a network footprint `(gw, gh)`
and an unobstructed rectangle `(w, h)`, maximize its occupied vertical span
`s × gh`, where `s = min(w/gw, h/gh)`. At fixed bearing and aspect ratio this
also maximizes the drawable network area `s² × gw × gh`. Prefer a full-height
network fit; unused paper alone is not the objective.

Complete rows/fields, the 5 mm inset and non-overlap are hard constraints.
`composition.min_font_scale` defaults to 1 (authored type). An explicit value
between 0.7 and 1 permits schedule type and row spacing to decrease by at most
30%. Equal-fit solutions prefer larger type before compactness and displacement.
The renderer tests up to four font tiers, eight measured panel/group shapes per
tier and a deterministic beam of twelve placements. This bounded heuristic does
not claim a global optimum.

Split panels need not have equal heights. `table.panel_rows` can specify exact
positive row counts for adjacent panels; the sum must cover every row, with no
duplication or omission, and each panel must fit its readable frame. With no
explicit counts, automatic composition considers balanced and stepped profiles.
Keys repeat above each panel. Only actual occupied panel rectangles obstruct
map fitting: the space below a short panel remains available.

A root’s `flow.after` followers move with it as one adjacent group. Panel heights
remain independent; grouping never pads shorter panels into a full rectangle.
List roots only in `composition.movable`, and all group members plus protected
titles/scales in the map’s `fit_around`. SVG furniture carries measured
`data-content-bounds` for proof inspection.

A local `report project export --print-layout <file>` proof replaces only an
existing selected layout on the same paper. The kernel verifies the original
server receipt first, changes only printing setups, and records the source
sheets digest plus exact local layout digests in each run. Engineering rooms,
model versions, project settings and saved project recipes remain governed.
These proofs are local-only, including with a release engine; `--publish`
refuses. This permits review before saving a recipe or deploying its renderer.

### Scale-aware line weights

A line's `style_overrides[layer].scale_weight` may declare a
`reference_denominator`, `min_factor` (0.25..1), and `max_factor` (1..4).
It requires an explicit base `size` and a governed line style. At the actual
fitted or authored camera scale, multiply that size and categorical sizes by
`sqrt(reference_denominator / denominator)`, clamped to those bounds. This
optical rule preserves category ratios and legibility; it does not scale text,
points, paper furniture or engineering geometry. The map and its legend consume
the same resolved pens. Omission preserves existing fixed physical pens.

### The collision contract

A sheet is a set of rectangles with meaning (`printing::diagnostics`). The
**furniture** — title block, legend, information table, schedules, scale bar,
north arrow, logos, texts — must not overlap one another, must stay inside
the **paper** less the **collision margin** (`composition.collision_margin_mm`:
0 allowed, 1 mm the preferred least and the default, 20 the most), and must
not cover the **design area**: the network's fitted footprint on the map,
which the engine measures from the projection. The map frame around that
footprint is displayed map, not a hazard — furniture may sit over it without
a finding — and decorative rectangles are not furniture (`role_of` decides:
a table bound to `lv_print_info` or presented as `workbook_info` is the
information table, any other table a schedule). Every violation is a typed
**warning**, never an error: `print_furniture_overlap`,
`print_design_overlap`, `print_element_clipped`, each with the overlapping
region, the roles, the overlap in millimetres, the element the suggestion
moves (the more movable role — schedules over legends and information
tables over title blocks; equal roles, the later one) and the smallest
axis-aligned move that clears it. A person or an agent reads them from
`print_diagnostics` (per named output in the engine result, the run receipt
and `report layout render`) and from the run warnings, and improves a
placement deliberately through `report layout edit`. `assess_furniture`
takes `margin_mm`; `furniture_roles` lists a layout's roles.

### Placement areas: the free surface as stepping rectangles

What is left of the printable area after the fixed furniture is an irregular
region (`printing::placement`). Its **maximal free rectangles** are where a
panel may go; its **skyline** from a corner is a series of stepping
rectangles, one per adjacent panel column, each as tall as the free space in
its column touching the anchored edge, skipping the corner furniture and
ending at the next blocked column, because panels are contiguous. A workbook
schedule is split across those steps — `panel_rows` gives each panel as many
rows as its step holds — so the schedule fills the irregular surface instead
of assuming equal panel heights. The engine offers these skyline splits to its
packing search beside the balanced and greedy ones, for every corner, so a
sheet whose title block, legend and information table leave a stepped
vacancy gets its schedules split to fit it automatically. `placement_areas`
exposes the free rectangles and the four skylines to CLI/MCP.

### Headless mechanical diagnostics

The Rust `printing::diagnostics` owner accepts measured occupied furniture
rectangles after composition. Native and WASM consumers share it through
`printingCommand`'s `assess_furniture` operation. It reports named intersections
and sheet clipping in millimetres with bounded details and exact totals.
Adjacent panels may have unequal heights; touching edges are not collisions.
Map frames and decorative backgrounds are not furniture obstacles.

The native renderer returns these findings with table omissions and suppressed
label counts in `print_diagnostics`; named report exports retain artifact
warnings. Cache integrity covers both rendered SVG and diagnostics. These are
mechanical checks, never a claim of visual or engineering approval.

### Transformer display names and project models

Plan titles and transformer labels use Python `str.title()` casing, owned by
`printing::names::transformer_title`. Separators are preserved (`upgrade_munyinya`
prints as `Upgrade_Munyinya`). Transformer text bindings and title/drawing aliases
containing that same name are normalized; separately authored drawing references
remain unchanged. This is presentation only: lookup keys, filenames, source
properties, model IDs, revisions and authorization context retain exact identities.
Native rendering and the WASM render plan consume this same rule.

Project DS Grid print context carries routes, structures and angle points from
the exact pinned models, with physical fallback styles for all three. An authored
metric corridor is derived from those routes at render time; it never replaces
model geometry. Existing rendered artifacts retain their original model snapshot.

Schedule panels consume their available printable height before continuing into
the next panel; the last panel may be shorter. Authored gaps remain physical
millimetres between panels and adjacent schedules. Consultant X/Y columns print
actual source properties, without inventing or changing their coordinate system.
