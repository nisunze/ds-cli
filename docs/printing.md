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

`ds.print-layout/v1` remains an accepted input only. Every evaluated V1 command
returns V2: standard paper is inferred from exact dimensions, other bounded
dimensions become custom paper, and the legacy map frame becomes the clipped
`primary_map` viewport. New output is never V1.

## Fixed legend anchors

`legend.align` chooses `left` or `right` within the authored legend frame.
The renderer measures the legend content and holds that edge as rows and
categories change. Omitted alignment preserves the historical `right` anchor.
Use `left` for a lower-left legend, remove its `flow`, and keep it out of
`composition.movable` to retain the authored position.

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
