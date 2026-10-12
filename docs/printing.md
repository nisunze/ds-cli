The owner ruling of 2026-10-02 requires API-resolved entity/source/target/role
style documents. [LV standard capture](printing/lv-standard.md) consumes those
exact receipts; it generates no pens, ids or typography. This supersedes older
implicit-style instructions below. `set_adjacent_networks` now requires the
complete explicit API-resolved ref map and refuses an omitted map. Missing
optional geometry does not require a reserved colour palette or label override.

# Printing document authority

An explicit `ds.design-output-selection/v1` delivery invocation consumes its
selected output inventory. A PDF-only map selection does not request, inspect
or export saved voltage-drop analysis and completes when all its selected
artifacts complete. Default/project LV report policy still includes saved
voltage-drop JSON; explicitly selecting it still names missing or invalid
analysis. The A4 voltage-drop PDF keeps its existing exact saved-analysis
companion. Reporting never recalculates that analysis. Native Desktop and
headless report hosts consume the same `report_formats::selected_plan`.

The 2026-10-02 style-resolution ruling supersedes any older instruction below
that constructs a style id, borrows a source sibling, or selects reserved pens
in code. See [governed style resolution](style-resolution.md): templates declare
the entity/source/target/role key, the shared kernel resolves the API snapshot,
and the renderer consumes the exact document and revision. Focused LV, adjacent
LV context and LV inside MV sheets keep independent governed documents. Pen
scaling, typography and emphasis remain editable database documents. The
before/after fixture raster comparison protects existing output.

`printing::Layout` is a closed `ds.print-layout/v2` document. Page dimensions and
frames use millimetres; typography uses points. It persists paper size and
orientation and contains one named, clipped print viewport,
text/legend/scale/north/logo/rectangle elements, held PNG assets and keyed
physical cartographic styles. The generated JSON schema is `layout_schema()`.
The map element's rectangle is the physical viewport. Its `view_slot` is bound
to a project named geographic view when an export is prepared; editor pan/zoom
is not part of this document.

An optional explicit `classification` owns catalogue organization:
`{purpose: "lv_transformer" | "mv_model" | "project" | "voltage_drop" | "front_matter", role: PageRole}`.
The shared page-role vocabulary includes `overview` and `report`. Front matter
holds only the booklet roles (cover, naming, sheet index, key map, key plan,
legend, general notes, legend and notes, revisions); validation refuses any
other role under it with `print_classification_invalid`. The purpose is
additive: documents classified under the four older purposes load unchanged. Omission is
Unclassified and serializes without this field; names, paper and the retired
`project_overview` flag never classify a layout. `set_classification` sets an
exact pair or clears it with null without changing any other layout content.
`template_catalogue` projects compact authoritative rows into purpose → role →
named templates and supplies the same native labels and authoring choices to
every consumer. Rows hold their saved paper metadata, independent of selection.

`elements` is the canonical back-to-front sheet stack. The final element is
front-most. `bring_to_front`, `send_to_back`, `move_above` and `move_below`
change only this order. Map-layer ordering inside the print viewport is a
separate authored `layer_order` list. `set_layer_order` changes only that list,
so administrative boundaries can be placed beneath the network without
changing geometry, selected sources, pens or page furniture.

Text elements may author an optional physical `text_style.halo` with a hex
colour and outward width in millimetres. An omitted halo keeps the historical
SVG and typography; project/purpose documents choose its appearance.

Design LV pole roles require their saved `struct_type`. A missing, blank or
non-text type is never inferred from material, numbering, colour or adjacent
poles. The native renderer keeps the exact geometry, marks each visible
untyped pole and prints a missing-type count, using the held renderer's
diagnostic ink and footer font. The placed `print_pole_category_missing`
warning names its design layer and source features. National reference poles
with a different schema are not retyped by this check. A category diagnostic
does not claim that a source repair or categorical style has been applied.

`printing::evaluate` evaluates New, Validate, Edit (upsert one element),
Remove, SetPaper, SetViewport and the four stack-order commands. Every result
contains `layout`; validation additionally returns
`valid:true`. Invalid edits produce no partial document. CLI and the web WASM
export `printingCommand` call the same evaluator. `command_schema()` describes
the closed intent grammar. The native renderer can validate the same Layout.

`template_catalogue` accepts custom-layout row facts and, optionally, the exact
project-scoped `report.standard.list` capture with its explicit `project_id`.
Every projected entry has a tagged storage `location`: `layout`,
`governed_html` or `governed_standard`. The bound `voltage-drop-a4-v1` document
is organized under Voltage drop / Report by its governed A4 contract. It is an
HTML document; its paper does not make it a canvas Layout. Governed standard
pages whose role is a front-matter role are listed under Front matter, named by
role and template, with a `governed_standard` location pinned to the project,
page id, template id and listed `revision_id` (read it with
`report.standard.get --kind standard`). A standard page without a known role,
template id or 64-hex revision pin refuses the capture rather than being
guessed. Standard pages of other roles keep their existing consumer (the
governed LV set) and stay out of the catalogue. Unknown custom layouts remain
Unclassified.

MV Sheets hold only the A3 landscape plan-and-profile sheet. `set_classification`
admits `mv_model` only as `mv_model / plan_profile` on a layout carrying an MV
plan and profile setup (whose validation fixes the A3 landscape page) and
refuses anything else with `print_classification_invalid`; district and
overview maps are `project / overview`. A stored pair is not re-judged on load:
a stored `mv_model` row with another role, or with saved paper other than A3
landscape, is listed as Unclassified with `mismatch: {code:
"print_classification_mv_requires_plan_profile", message, stored}` until it is
reclassified. Every admitted entry carries the preview `subject` its purpose
previews against (`transformer`, `mv_models`, `extent` or `layout`). Static front
matter roles (cover, naming, sheet index, legend, general notes, legend and notes,
revisions) consume a captured layout page; they do not require MV inventory,
transformer design or a geographic extent. Key map and key plan retain their
explicit extent requirement. Governed standard reads return the same native
role-derived subject and preserve the original authored document and pins.

`report.layout.render --preview` consumes one `ds.print-layout-export/v1`
capture through the shared native file-preview owner. SVG is the default;
`--preview-format pdf` selects one PDF through the same fixed local
`render-print-layout` producer. The closed format choice joins the exact scope,
layout, complete input capture, governed source pins and engine build identity
in the reuse key. Both return only the common disk artifact manifest, with the
24-hour or declared seven-day lifetime; reuse keeps its original expiry.
Concurrent consumers render once. Preview and PDF download read the admitted
file through the same digest-fenced reader, and PDF download follows visual
inspection of that PDF. No inline producer or alternate preview route exists.

A governed capture may carry `source: {kind: "governed_standard", location,
document}`. Native admission checks project, exact catalogue revision/content
pins, role and equality of the captured layout to that original document before
the producer runs. The reuse key hashes this source. The fixed producer receives
the admitted layout, omitting only declared, unbound logo placeholders; the
original governed source remains intact. No model, style or project provider
is available to this host. Desktop captures retained governed style/default
policy; a missing or invalidated policy names an explicit acquisition need.

`catalogue::from_listing` is the one grouped catalogue read
(`report.layout.list --scope project --project <id> --catalogue`). It takes the
printing list's `setups` and, when it could be read, the project's governed
`report.standard.list` capture, and returns the `template_catalogue` groups with
`project_id` and `governed: {read: true}` or `{read: false, code, message}`. A
governed read that failed keeps the custom layouts listed. ds-cli and the
Desktop door (`ds_invoke` `report.layout.list`) both answer it: the Desktop
shell performs the two reads that `ds_client_core::HeldSessionPrintingRead`
plans, under its session credential, so the Print templates panel renders the
same JSON a terminal reads and keeps no catalogue cache of its own. The panel's
A4 source read is `report.standard.get` through the same door.

`read_governed_template` admits the exact `report.standard.get` response against
the selected HTML location's captured project, revision, content and HTML pins.
It returns the original governed HTML bytes. It neither injects example data
nor renders a saved voltage-drop result; that projection belongs to the native
reporter. A source view alone does not prove a populated preview or PDF.

Spatial hit testing, snapping and retained camera sessions remain in ds-network;
they are not command-kernel WASM exports. PDF/SVG formatting, GeoJSON projection
and legend rendering belong to ds-network-reporter. Brain owns global/project
sharing and expected-revision saves. This module owns no storage or network IO.

## Individual saved voltage-drop analysis

Every individual LV report includes `voltage_drop`, the exact saved producer
JSON with the suffix `.voltage-drop.json`. An explicit project selection or
local override cannot omit it. Missing or stale analysis remains a report
blocker; reporting never runs the solver or repairs the saved analysis.

The effective policy keeps this artifact available on both declared data-report
hosts (`desktop`, including the headless Linux Server, and `web`). A historical
execution entry that suppresses this mandatory artifact cannot disable it.
The selection, execution matrix and voltage-drop presentation choice remain
stored as authored; other outputs retain their placement rules. The separate
A4 `voltage_drop_pdf` remains optional and uses the local print runtime.

## Combined report output

The Combined Report is a project-wide data export. It always selects SHP, XLSX,
and GeoJSON sequence, with GPKG and Network Information when selected. A saved
PDF selection applies to individual transformer reports only. The Combined
workbook and geospatial outputs remain one project-wide set.

The owner revised the print-delivery requirement on 2026-10-02. A canonical
LV combined delivery must include a district/project MV A0 overview with
project statistics. This supersedes the earlier blanket prohibition on project
overviews and combined print delivery. Rendering runs only on a PC, including
the signed-in Linux Server; cloud compute remains data-only. Each transformer
sheet and the overview are bounded local jobs with exact project/model/layout
pins. Package their admitted artifacts without rerunning engineering or
pretending that a compounded data ZIP is the requested print set.

The current combined data task does not produce this overview or a complete
print collection. That remains an implementation and delivery gap: neither a
saved PDF selection nor a successful data task proves that these pages exist.
The legacy `Layout.project_overview` boolean remains ignored; the new overview
must be a named governed printout with its own layout and artifact identity,
not a hidden effect of an individual transformer layout. Verify exact online
publication independently of local rendering and outbox reclamation.

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

## LV in two print modes

LV network content prints in exactly two modes. They differ in purpose, palette
and pen authority, and neither borrows the other's treatment. Mixing them is
the most frequent printing error, so this section is the authority both hosts,
the CLI skills and delivery instructions defer to.

| | Standalone LV transformer sheet | LV inside an MV plan/profile sheet |
| --- | --- | --- |
| Purpose | the issued drawing of one transformer's LV network | context that places the MV route among the LV it supplies |
| Main content | the LV network; geography is subordinate | the MV route, structures and profile conductors |
| LV palette | categorical engineering palette: lines by cable size, poles by structure type, existing and new apart; every drawn category named in the legend | one bluish family graded by importance (new LV design darker; customers and existing LV lighter), quieter than the red MV pens; no categorical palette |
| Furniture | pole and customer schedules, Network Information, categorical legend | no LV schedules; MV structure labels and staking facts |
| Pen authority | declarative `Layout.style_overrides` over the exact resolved focused documents | independent documents resolved for `role: mv_context`, held by exact revision in the MV context print capture |

- An instruction scoped to one mode never changes the other. "Uniform bluish,
  even customers" is an MV context instruction and never recolours a
  transformer sheet; a cable-size or structure palette is a transformer-sheet
  instruction and never reaches MV context.
- A standalone sheet whose LV lines and poles print in one flat colour is a
  defect unless its template deliberately authors monochrome and the legend
  says so. Deliberate monochrome stays valid.
- Acceptance inspects one dense page of each mode at paper size: on the sheet,
  every drawn category appears in the legend; in the booklet, LV reads as blue
  context beneath the red MV route.

## Canonical next-export defaults

### Network hierarchy

Printing keeps three independent facts: **role**, **lifecycle**, and **scope**.
MV and LV conductors remain different layers and pens. Transformers, main LV
circuits, service connections, customers and poles each retain their governed
symbol, casing, dimensions and line pattern. The presence of a new transformer
does not make its connected LV lines, poles or customers new. A neighbouring
network is context, not evidence that it is existing. A model or survey source
does not establish an asset's lifecycle.

| Fact | Authority | Print behaviour |
|---|---|---|
| MV versus LV and asset role | Canonical source layer and its governed print style | Separate line/symbol semantics; never classify ABC as a service cable |
| Proposed versus existing | The feature's explicit `existing_status` (`new`, `existing`) | Independent categorical colours; preserve component-level `existing_status_detail` |
| Absent or other lifecycle value | The unchanged source value | Explicit unclassified fallback; never infer from ownership or map proximity |
| Focus versus neighbouring context | Exact transformer identity and saved room version | Focus stays primary; neighbouring circuits render underneath with subdued pens |
| Engineering verdict and quantities | The pinned producer run for the selected scope | No verdict or schedule inheritance from visible context |

`PrintStyleOverride.palette_fallback` sets the colour for an unknown category
without flattening a palette. It requires a palette; `color` and `palette`
remain mutually exclusive. Absent fallback preserves existing documents'
governed fallback. `color_scope: "body_and_boundary"` applies the effective
colour and opacity to an existing outline/casing without changing its width,
icon, coordinates or line pattern. The default `body` preserves legacy pens.
The renderer and legend use the same resolved style document.

`resolve_styles` accepts a captured `ds.style-resolution/v1` API table and
an exact `style_keys` map, invokes the sole kernel resolver, and returns the
updated layout plus held `print_styles`, `style_revisions` and the resolution
revision. A renderer consumes that held capture without another catalogue read.
There is no inferred style id or source-kind fallback.
The resolver key optionally declares `ink: "colour" | "monochrome"` for
paper. Omitted ink and explicit colour are the same key; omission retains the
original four-dimensional wire representation and document digests. Monochrome
is an independent governed document binding, never renderer recolouring.
Unknown ink combinations are refused by name; monochrome on screen is invalid.
The standard LV consumer's singular classes (for example `lv_line`, `customer`
and `lv_transformer`) are explicit seed declarations alongside existing layer
classes, across the declared source kinds and focused/context/project roles.
The governed `Layout.style_keys` declares those choices per layer. The operation
uses it when the request omits `style_keys`; a legacy template must acquire its
exact keys through an API document migration. A host never reverses an old
style ref to guess source kind or print role.

`set_adjacent_networks` atomically binds all five neighbouring roles to their
API-resolved print references. Empty `style_refs` refuses with
`style_resolution_required`; a map must name exactly `tr`, `lv_lines`,
`service_cables`, `customers` and `lv_poles`. Its optional closed
`ds.adjacent-print-hierarchy/v1` policy
contains proposed, existing and unclassified colours plus opacity. When absent,
the named governed style owns those values; the kernel does not replace them
with a hard-coded palette. Brain packages the versioned create-only starter
registry and exposes it in Style Center through the existing seed/print-clone
commands. Existing authored screen, tile and print documents are preserved.
Client palettes replace the starter through a governed print-style edit or an
explicit project-local policy. No client-specific blue becomes global. Every
render receipt pins the exact effective styles and source revisions.

Context keeps the fields read by effective governed symbols and lifecycle pens.
Label visibility belongs to the resolved context documents: the standard
customer/line/pole documents have no labels; transformer identities can remain
labelled. Configuration and field projection no longer erase an authored label.
It reads all active, checksum-verified saved rooms once and clips their exact
geometry to the focused viewport, including edge-crossing circuits whose
transformer is outside it. It does not widen the focused camera, alter topology
or import context into quantities, customer schedules or electrical reports.
Staging first selects whole features intersecting the captured sheet coverage;
the renderer performs the final clip. Off-sheet transformer points do not
exclude crossing circuits. Each staged layer names its projected feature count
and the complete saved-room revision/digest roster in its source receipt.
Held preview uses the same projection and explicitly names unavailable context.
Partial context pen sets, wrong ownership, missing style documents and incomplete
rosters refuse rather than silently guessing.

Published global samples and project-adopted revisions are the starting point,
not an LLM's last temporary recipe. Keep MV front matter, ordered pages, issue
fields and general presentation in the same project printing library, editable
and previewable through the native CLI/MCP surface. Input acquisition runs only
for an operation actually consuming those inputs under a captured
principal/project. A preview and its read-only renderer consume held context,
name missing inputs and never acquire providers. Explicit acquisition retains
its existing owner and project fences.

The preferred MV reference is the October 1 Nyamagabe Model 1 A3 booklet:
`Nyamagabe-MV-Model1-Plan-and-Profile-A3.pdf`, 100 A3 landscape pages,
SHA-256 `13e82d117c84c0b80b2cff271de3927a1ca6335bc09a967a3f7ed7d6c689d9cb`.
Its approved MV wire hierarchy is independent of secondary LV/customer context.
LV keeps the two print modes defined in [LV in two print modes](#lv-in-two-print-modes):
an MV background treatment never replaces a transformer sheet's categorical
palette. Project-specific parties, logos and contract facts are not global
product defaults.

MV structure labels use the existing staking table's authored Structure
Description alongside the existing structure name. A placed description takes
precedence over the resolved type description, exactly as in staking Excel;
an explicitly empty placed value does not invent a library fallback. Preserve
earthing, DOF, transformer and special tags. Derive no extra pole, height,
material or stay wording in the renderer. Wrap and paginate complete names;
never report an omitted-description count as a complete drawing. Special
feature markers, captions and clearance hairs start
at the admitted ground-offset guide. Native surveyed elevations, clearance
thresholds, guide gaps and governing calculations remain unchanged.

Staking tables are Excel deliverables and never printed pages. The canonical
native PLS report set is Structure Usage, Section Usage, Section Sag-Tension,
Summary and Terrain Clearances, each RTF plus A4 PDF. Explicit customization
may request other supported outputs without rewriting the project's baseline.
These native reports do not authorize PLS-CADD plan/profile sheet printing.

## Fixed legend anchors

`legend.align` chooses `left` or `right` within the authored legend frame.
The renderer measures the legend content and holds that edge as rows and
categories change. Omitted alignment preserves the historical `right` anchor.
Use `left` for a lower-left legend, remove its `flow`, and keep it out of
`composition.movable` to retain the authored position.

A categorical legend may author `category_fields` by logical layer: up to three
ordered, distinct source fields per layer. Observed combinations each retain a
row resolved through the same held style as the map. This separates type,
material and lifecycle without inferring any engineering fact. Missing or empty
values read as `Unspecified`; structured values refuse. Unmentioned layers keep
their existing primary-colour category behavior. The empty setting is omitted
and retains historical bytes. Category and total-row bounds remain unchanged.

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

The native search is deterministic and bounded; it maximises the fit among
the candidates it examines, rather than claiming a global optimum.

### Network footprint and project scale limits

`network_fit` opts a layout into stepped network fitting. Its `source_layers`
explicitly name the engineering layers that must remain visible, rather than
context such as national roads, buildings or administrative polygons. Missing
or empty required sources refuse the render. Geometry is never edited.

One continuous projection, with the existing bearing, fits the complete source
footprint around measured furniture inside the thick printable border. A
conservative 16-by-16 occupancy grid includes line segments between vertices
and polygon interiors; contiguous occupied cells form stepping rectangles.
`clearance_mm` expands those rectangles for symbols and nearby labels. It is
an explicit physical clearance, not a guarantee that every optional map label
can be placed. Mandatory engineering values remain in complete schedules.
The same rectangles drive fitting and furniture collision assessment.
Fitting retains the named view's padding (4% for an automatic view) and the
existing 0.0001-degree minimum geographic pad for degenerate sources. A single
point must not create a zero print denominator. GeometryCollection sources
retain their component geometry and filled-polygon protection.

Composition still honours explicit corners, perimeter/shelf placement,
readable type, complete rows, full-height-first panels and the measured gaps.
Tables never acquire a shared global container. The map may use vacancies
between them while their independent parent-relative frames remain intact.
The rectangular fitter remains the default when `network_fit` is absent.

`scale_limits` is a template policy shared by the project's transformer sheets.
`min_denominator` is the closest allowed view (most zoomed in), and
`max_denominator` is the farthest allowed view (most zoomed out). Both are
within 10..100,000,000 and the minimum must not exceed the maximum. No numeric
limits are invented for existing projects. A small design stops growing at the
minimum; a design that cannot fit completely before the maximum refuses with
an actionable scale-limit error. It is never cropped to satisfy that policy.
Automatic scale rounding happens before the maximum check; an explicit camera
keeps its exact scale but must satisfy the bounds. It cannot be combined with
automatic stepped fitting. Omitting both policies preserves existing behavior.

Native assessment records the fit mode, resolved scale denominator, whether
the closest-scale limit capped enlargement, and the occupied footprint. CLI
and MCP author these policies through `set_fit_policy`; the UI adapts that
same operation. A draft proof is not an online publication receipt.

`preview_context_preparation` describes selected draft context with
`acquire_missing:false` whether connected or offline.
`context_status` presents omissions: an acquired layer with no features in the
design extent is neutral; an unheld/unavailable source retains its reason.
This does not mutate source holdings or the saved project selection. The UI
invalidates old render diagnostics when a draft changes and can group map-data
or pen edits before one explicit preview. Project persistence uses the existing
revision-fenced full-template save, separate from trying a transient draft.

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
| service_area | Service Area Length M |
| village | Village |
| x | X |
| y | Y |

**As-built-only fields are omitted when the project is not as-built.** These
identify an installed, metered connection and carry no meaning for a design:

- `meter_type`

**Personal identity never prints** (owner, 2026-10-07). National ID (`nid`,
`national_id`) and phone numbers are excluded from every external report
projection, as-built included; the schedule follows the workbook, which never
carries them.

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

`table.heading_mode` is `auto_index` (default), `literal`, `indexed`, or
`abbreviated`. `auto_index` aliases only titles wider than their data-sized,
min/max-bounded columns; titles that already fit remain written out.
This avoids long titles such as "Service Area Length M" setting a numeric
column's width. The aliases skip retained literal letter titles so the key is
unambiguous. `indexed` aliases every column. `abbreviated` may use a measured
prefix with an ellipsis, falling back to a letter when necessary. Compact modes
repeat an untruncated explanatory key below each
panel. Full headings remain in the document; columns and values never change.
The full key repeats below each measured data panel, wrapping at the authored
type size within that panel width. It never extends into unused authored frame
width or creates a gap before an adjacent schedule. This corrects the former
full-frame key behavior, which visibly separated the Zihare schedule bodies.
The key consumes actual table height;
row omission warnings must use that same remaining height. `measure_headings`
continues to govern literal headings; compact headings measure their displayed aliases.
Per-transformer `table.heading_mode` overrides the project layout through the
same existing print-instruction command. Old documents omit the default field
when serialized, preserving compatibility and their existing presentation.

## The shared preview page

`printing::view::plan` is the read-only consumption authority for the open
Printing templates view. `report.layout.edit` exposes its `view_plan` intent
headlessly. It receives the explicit project, exact held catalogue locations,
current session state and actual held document page count. It admits one exact
location or one held page, refuses replacement of dirty drafts through the
existing session reducer, and returns no acquisition, refresh, render or save
effect. Same-template reads and page navigation remain usable with dirty work.
The Desktop door serves only this pure intent under the owner window's existing
account/project route; the paired `desktop.printing.view` transports the same
request. PDF navigation consumes all held pages. A one-sheet MV sample remains
one held page until the renderer declares an exact requested-sheet contract.

Desktop native transformer and MV template previews retain their latest exact result in
the existing account/project report hold through `ds_project_data::preview_room`.
The host captures account, project, lane and credential audience before reading
the room. Each consumer still admits the layout, governed input receipt, styles
and actual held context. The room key pins their exact bytes and the verified
reporter build; changed inputs render a new result. Repeated navigation and
overlapping consumers reuse that result through the retained-read owner, with a
file lock for another native process. The room performs no provider acquisition,
cloud read or freshness probe. Invalid local bytes are recomputed by the next
intentional preview; a failed render preserves the previous valid result.

Navigation releases only the canvas view. An admitted native render continues,
stores its original scoped result and can be consumed when the user returns.
The view checks its current account, project, template and subject before
painting, and a preview spinner never disables template or camera navigation.

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
from facts: the draft, the chosen `subject`, the governed print styles the
host holds, online and native-engine readiness. The subject follows the
layout's purpose (`expected_subject`): LV sheets and voltage-drop reports
preview against one transformer as `printable_transformers` listed it
(`{"subject":"transformer","name",...}`; the legacy `transformer` fact reads as
that subject); MV sheets and front matter against one or more DS Grid MV models
(`{"subject":"mv_models","model_ids":[...]}`); overview sheets against a map
extent (`{"subject":"extent","bbox":[w,s,e,n]}` or the project extent). An
unclassified layout carrying an MV setup reads as an MV sheet, any other as a
transformer sheet. A subject of another kind is refused with
`printing_preview_subject_mismatch` naming the expected and given kinds; an MV
template never previews against an LV transformer. The plan names the expected
`subject` and the canonical `render_command` for it (`report.project.export`
for a transformer, `report.plan-profile` for MV models); no native renderer
previews an extent yet, so an extent subject is refused with
`printing_preview_extent_unavailable` rather than rendered around a
transformer. Refusals are keyed (`printing_preview_no_transformer`,
`printing_preview_no_mv_model`, `printing_preview_no_extent`,
`printing_preview_subject_mismatch`, `printing_preview_mv_model_invalid`,
`printing_preview_extent_invalid`, `printing_preview_extent_unavailable`,
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

Native SVG and saved-analysis A4 PDF previews share
`ds-report-host::preview_artifact`: one `ds.print-preview-artifact/v1` file
manifest, scoped account/project/lane/audience room, byte/hash admission,
24-hour default lifetime and optional seven-day lifetime. Opening a retained
file never renders, acquires inputs or observes remote freshness. Reuse keeps
the original expiry. Files are the output; renderer responses contain only
their small manifests and diagnostics. The shared reader refuses crossed
authority, paths, expired files, wrong SVG/PDF media and changed bytes.

The fixed A4 producer admits the exact saved analysis before a cache lookup.
Its common disk input lease binds the transformer and saved revision, original
analysis SHA, layer content digest, captured settings/selection and exact engine
build. Overlapping consumers join one renderer task; restarting or returning
to an earlier exact input hash consumes its disk result. Navigation releases
the view's adoption only; the admitted native task retains its completed file.
No missing analysis is replaced by an older preview or recomputation. A4
delivery's existing saved JSON companion remains an exact source artifact;
it is not a second preview path. Existing main publication and retained report
downloads keep their separate contracts.

The paired CLI consumer is `desktop printing export --intent preview`
with only `voltage_drop_pdf` selected and optional
`--preview-retention 24h|7d`. Its result exposes the common `previewArtifact`
and a native reuse receipt. Server's `report project export
--preview-voltage-drop` admits its own exact scoped native hold and saved
analysis before using this same owner. It branches before project/provider,
tag, survey, freshness or publication reads. The headless response returns
the same common PDF file manifest and original lifetime; missing or moved
analysis refuses before scratch or renderer invocation.

`ds-report-host::layout_preview::execute` retains one inline
`ds.print-layout-export/v1` page through that common input lease and SVG file
owner. The namespace is admitted independently by the native host; captured
JSON never grants account or project authority. An included project-CRS
capsule must match project, principal, lane and audience. The exact authored
capture, including source digest metadata, layout, styles, symbols, config
and held renderer policy, participates in its input hash with the renderer
build. No style or geometry is inferred. Only delivery output transport and
format are replaced by the explicit SVG preview mode. One fixed
`render-print-layout` task writes private scratch; its bounded manifest,
layout/paper identity and exact SVG bytes are verified before common
materialization. Missing policy, multipage/file-indirected captures and
private page caches refuse before rendering. Cold and warm producers expose
only the shared file manifest, reuse receipt and bounded diagnostics.
Each cover, naming page, index or overview supplies its own exact captured
page through this owner; this does not introduce another preview store.

The inventory carries exact saved `layers.customers` and `layers.lv_poles`
counts when present. `preview_cases` identifies the smallest and largest of
each among the returned rows, with stable name order for ties. Missing or
noninteger counts remain absent; `more.omitted` states when this is a partial
inventory.

Each row's `voltage_drop` describes its held saved-JSON candidate, separately
from map availability. `saved_json` requires a positive exact server version,
matching server/receipt content digest, the admitted analysis schema/method,
all four byte/producer digest pins, bounded JSON size and a consistent summary.
Its verdict is the producer's `complies`, `complies_with_reinforcement`,
`does_not_comply`, `incomplete`, `refused` or `reserved`; no voltage threshold
or recommendation is calculated by the inventory. `rule_confirmed` passes
through the actual producer value without an assumed default. Missing, stale,
malformed and locally edited rows cannot offer saved JSON.

This projection does not authorize rendering by itself. The consumer must
freshly read the exact saved analysis under version, content and analysis
pins, and admit its current nature/configuration binding before rendering.

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
Keys repeat below each panel. Only actual occupied panel rectangles obstruct
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

Plan titles and transformer labels use presentation casing, owned by
`printing::names::transformer_title`. Underscores become spaces (`upgrade_munyinya`
prints as `Upgrade Munyinya`). Transformer text bindings and title/drawing aliases
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

### Printable border and independent schedule footprints

The canonical `page-border` rectangle is the printable domain, independent of
map viewport and physical paper. Templates without it retain the page domain.
Native SVG, PDF and raster output clip every page element to that domain and
paint the border last; it cannot be hidden beneath furniture. Edge anchors and
composition use the domain's inset, and diagnostics name clipping against this
same domain in absolute sheet coordinates.

Each schedule is its own element with independent authored bounds and measured
panel footprints. Linking two schedules through `flow` explicitly couples their
placement; independent schedules have no flow link. Automatic placement retains
stepped footprints, so vacant space below a shorter final panel remains usable.
At authored type and row spacing, automatic schedules fill available vertical
capacity before opening another panel; balancing or reversing panel splits is
not an automatic alternative. An explicit `panel_rows` recipe remains authored.

`sheet_frames` publishes the shared parent-relative hierarchy: map space owns
its bordered domain, the border owns each independent table container, and the
container owns a table with local origin (0, 0). Existing V2 absolute coordinates
are converted at this adapter boundary only. Table rendering uses its local
container coordinates and clip; the parent supplies the single translation.
Measured panel footprints remain separate children, not a padded shared region.

`table_container_rect` edits a table container in border-relative millimetres.
Rust resolves its parent offset once, retains a local table origin of zero, and
clears sibling flow for an explicitly positioned independent container.

`table_drafts` returns compact editor schematics at authored physical row/font
sizes. Heading widths are approximate; they never stretch to maximum container
size or become export geometry. Reporter alone measures real printed content.
`print_label_omissions` is a scoped zero-row warning, accepted by the shared
result validator like diagnostics summaries; wrong run/output/layout is refused.


`sheet_status` presents verified assessment facts consistently to CLI and UI.
Polygon-containment, spacing, frame-clipping and collision label omissions are
informational `label_notes` with counts and plain-language reasons; they concern
map text. Actual furniture findings and missing schedule rows remain warnings.
Repeated output assessments do not multiply one sheet's label counts. The raw
renderer diagnostics and governed warning validation remain unchanged.

## Screen and print identity; context alternatives

Governed print refs have target `print` and the `_print` identity. A screen
style can seed an independent print copy once; the two documents do not share
later edits or preview routing. Symbol assets preserve their SVG winding rule
(default nonzero), intrinsic untintable fills and aspect ratio; foreground paths
never inherit a line/polygon stroke from their surrounding map layer. Inspect
symbols and legend entries at physical size in the SVG/PDF.

For Rwanda's initial context selection, prefer HV/MV/LV over `powerlines` and
primary/secondary schools over `elementary_school` when all detailed alternatives
are held or downloadable. These are presentation choices, not proofs of equal
source coverage. Alternate datasets remain selectable, and missing detailed
classes keep the broad fallback selected. Catalog rows explain omitted defaults.
Existing saved selections remain authored; use the existing context toggle and
revision-fenced project save to remove extra representations. Group headings
are not feature layers. Do not delete source data or modify interactive-map
styles when cleaning print context.

### Centred map composition and optional transformer focus

Automatic stepped fitting first maximizes its bounded feasible scale under the
saved scale limits. At the same scale it chooses the complete footprint nearest
the centre of its printable parent, respecting furniture and clearance. The
candidate set includes the rectangular baseline so recentering cannot sacrifice
coverage merely because that earlier fit was better than grid trials. This is a
bounded deterministic search, not a global packing or centring optimum.

`set_focus_visibility` authors `focus.visible` through the same CLI/MCP layout
intent used by the UI. Omission remains visible for existing templates. False
hides the focus shade and its outline while retaining its colours, opacity and
buffer. It does not alter engineering layers, geographic coverage, scale or
schedule placement. Enabling requires configured focus styling. Draft preview
and project-template save retain their existing separate steps.

Plain map schedule columns default to `column_sizing: content`: each uses its
own formatted values and headings, with the existing physical padding and
explicit minimum/maximum constraints. No coordinate field names are special.
An optional `proportional` policy retains authored width weights. Workbook
information/Excel presentation retains its existing proportional styling.
Precision, complete rows, measured panel geometry and table parents are
unchanged. White space is not introduced to equalize unrelated columns.

### Lossless compact column titles

The default `heading_mode: auto_index` keeps fitting titles written out and
aliases only long titles: for example `A, Description, B, X, Y`, with
`A: Number` and `B: Flying stay` in the full key below every panel. Literal
letter titles are reserved before aliases are assigned, so an existing `A` or
`X` cannot become ambiguous. All full definitions remain in source order.
`indexed` labels every column; `literal` keeps authored titles. Optional
`abbreviated` uses a measured prefix/ellipsis with the same full definition key,
falling back to a letter when a prefix cannot fit or would be ambiguous. The
native renderer owns font measurement; UI and CLI author the same closed mode.

## Canonical MV publication inheritance

`Layout.mv` extends this same library document with ordered `layout`,
`preserved_pdf`, `model_preserved_pdf` and one `plan_profile` geometry slot.
`printing::mv` owns closed selection/resolve/refusal schemas; `mv_settings`
owns presentation extracted from the existing reporter, with no geometry or
engineering dependency. Adopted templates explicitly declare every sheet choice,
required text, permitted identity/title fields, ordered embedded logos and the
bundled font. Ordinary LV documents omit `mv` and retain their wire bytes.

One `mv_printing_setup` project parameter pins a project library id/revision,
simple external publication number and fixed real calendar date. Resolve
requires the exact selected revision. Copied setups retain their exact source
project (or historical global source), id and revision; a directly authored
setup captures its own project/id/revision. Resolution never reads a mutable
source head. A `model_preserved_pdf` binding names each permitted exact
model identity with its approved asset id/digest. The resolved receipt records
each chosen preserved page by index; a consumer re-resolves and compares the
whole receipt, so changing a cover pin, project, issue or scale refuses. Model1
and Model2 can share the same document and issue while preserving distinct covers
and a fixed shared naming page. No fallback or inferred document text exists.

ProjectSettings accepts explicit `project_id` for MV inspection. Configured
documents are never printing readiness: model geometry and exact held PDF
bytes remain required, and `ready` is false. MvSelectionPatch preserves other
settings rows. Native persistence adds an exact-base expected-data fence and
readback; adapters own IO, never layout decisions. Reporter renders the shared
layout furniture and consumes the resolved ordered page pins before output.

## Standard MV booklet consumer (2026-10-02)

`mv_standardize` canonizes an approved cover/naming pair in the existing global/
project printing library, then adds sheet index, key plan, legend/general notes
and the native drawing slot. It preserves optional approved context documents.
A3 landscape, H1500/V500, vertical native identities, a fixed external version/
issue date and the guide-anchored clearance hair are the standard. Required
formal parties/contract and exact approved logo/PDF assets refuse when missing.
Overrides are declarative revisions of that Layout, validated by the kernel;
the renderer accepts no transient furniture or style choices.

`mv_booklet_plan` seals ordered page roles/content with project, model revision
and adopted printing revision. `mv_booklet_preview` admits each ordered PNG
against that plan, checks dimensions and encoded/decoded hashes, and returns
portable data URLs. Native and `printingCommand`/WASM admission share this code.
The native reporter rasterizes finalized component PDF pages, including the
existing footer owner, before assembling the booklet. Preview-only omits the
assembled PDF; every PNG and plan must equal the later full render.

The MV consumer needs the print-styles resolver binding for
`(mv_booklet, project_model, print, project)`. Its MapLibre document must carry
`metadata.mv_paper` (`ds.mv-paper-style/v1`): physical primitive bindings for
pens, fonts, label sizes, halos, casing/dashes, symbols and generated index
 typography. `metadata.print_scaling` and optional `metadata.scale_weight`
use the existing kernel policies. Primitive expressions receive governed
SheetSettings and exact native feature/cable properties; ink policy lives in
those expressions. The synthetic baseline-value reply is test data, not a
seed, catalogue entry, or production fallback. Print-styles owns API seeding.

Geographic captures carry explicit per-layer source queries and exact resolver
bindings inside their source digest. Required combinations include LV lines,
poles, transformers and existing/proposed MV, with live_geojson/vector_tile/
cold_existing/dataset_geojson sources and focused/context/mv_context/project
roles as declared by acquisition. Rivers, roads, boundaries, places and model
zones likewise require their actual source/role. MV never constructs style ids,
seeds zone pens or post-rewrites captured colours. Main integrates acquisition
of these replies; absent or changed bindings refuse. Share these generic query/
binding and page-role contracts with LV; do not copy either renderer's paints.
## Governed standard document captures

`printing::standards` consumes `ds.print-standard/v1` documents captured through
an authorized project API. Each `(template_id, page_role)` document pins its
complete layout, ordered logo slots and semantic resolver keys by immutable
revision and normalized content digest. Native and WASM instantiate the same
held document. No layout generator, typography seed, pen catalogue or style
selection belongs to this consumer. Missing or ambiguous captures and stale
content have named refusals. Document-level label controls may bind to declared
semantic layer keys before style references exist; this is validation of the
API printing document, not style selection. Rendering still requires the exact
resolver receipt and resolved reference for every active key.

Every standard layout also holds `attribution`: its physical type size, SVG and
native PDF font families, colour, right inset and baseline. These are governed
printing-document values, with no defaults in the standard consumer. Licensing
content remains mandatory once per page. The SVG carries this same policy into
PDF conversion; assembly preserves an already stamped page. Older layouts
without this additive capture keep their pre-existing native PDF stamp.

The Styles API capture boundary is `printing::resolved_styles::consume`. Its
input is the sole resolver's `prepare_print` receipt plus the exact request
keys. The consumer validates the project, source/target/role and immutable
style revisions and forwards the original documents unchanged. LV needs all
canonical LV entities with `focused`, `context`, and `project` print roles,
for every explicit source kind present, plus MV context, buildings, contours,
roads and rivers. Colour and monochrome require API-selected governed variants;
the resolver owner must extend its owning contract if ink is not yet a captured
dimension. Physical scaling and labels remain in the resolved documents.

## Report language and artifact names

The existing Network Template `project_settings` sheet owns `report_locale`.
Firestore groups it under `parameters.project_settings`; the project `/config`
projection flattens the same rows. Exactly one explicit catalogue key is required.
No country, operator UI language or absent-value default selects a report language.
The dry-run-first [backfill](../migrations/report-locale-and-artifact-names.md)
adds a reviewed value to existing projects and the Network Template; it is not run
as part of rendering.

`report_messages` owns `ds.report-messages/v1`, whose one default catalogue is
[data/report-messages.json](../data/report-messages.json). Each locale supplies
exactly the English reference key set, nonempty values and identical named
placeholders. `en` and `fr` ship; another locale is a document change. A project
may supply the same complete document as the existing JSON-valued
`project_settings` parameter `report_messages`; the project API preserves this
value. Unknown object sheets are not a served catalogue source.
The same document carries `script_code_literals`: authored template protocol IDs,
selectors and enum literals that are preserved during HTML materialization.
Overrides retain this required set and every governed reference message; all
locale dictionaries are complete and preserve placeholders. Captions and code
literals may have the same spelling without changing program behavior.
`report_messages::Messages` validates it. Named refusals are
`report_locale_missing`, `report_locale_invalid`, `report_locale_unsupported`,
`report_catalogue_invalid`, `report_catalogue_incomplete`, `report_message_missing`
and `report_header_alias_invalid`.

The template's existing header-mapping sheets (`customers_header`, `lv_lines_header`,
`lv_poles_header`, `service_cables_header`, `transformer`) use `cleaned_header` and
`misspelled_header`. `template_headers` reads flat/grouped API forms and builds
one canonical-field/alias map; `header_alias` is the sole consumer function.
Ambiguous project aliases refuse. Canonical nonempty data wins over an alias.
Catalogue headers preserve the former governed compatibility aliases centrally.

`report_formats::report_filename(stem, format)` owns every report filename.
PDF stems use the protected word-boundary title casing (`Run_Ruhuha.a0.pdf`);
exchange and raster stems preserve exact identifiers (`run_ruhuha.xlsx`).
Individual voltage-drop PDF stems retain the established human spacing.
Named layout suffixes and media types come from `report_formats::output`.
`report::Command::ArtifactFilename` exposes the same rule to WASM/web. Artifact
inventory, project-control downloads, reporter exports, local readers and web
promotion consume that rule. An inconsistent stored name needs the reviewed
backfill; readers never probe a second spelling.

### Governed paper scaling migration (2026-10-02)

The renderer now honors every stored `metadata.print_scaling` factor exactly.
The historical ISO-area table no longer triggers a code-side substitution to
identity factors. The standard seed manifest records explicit exact-head
migrations for installed legacy tables to the currently effective all-one
factors. A fresh install receives those governed values. Existing installations
must review and apply the versioned migration through the style API before
releasing consumers; authored changes are never overwritten. Immutable source
versions and backups retain the original tables. Baseline renders use the old
read-only lane and original documents; branch renders use the migrated governed
documents. The migration must preserve their page rasters.

A line document may declare `metadata.scale_weight` with the same closed
`reference_denominator`, `min_factor` and `max_factor` policy as a template
override. `ResolveStyles` captures that policy and the document's scalar base
pen into the shared map/legend override path. Explicit template policy wins.
A categorical or zoom width expression requires an explicit declarative scalar
base-size override for this optical policy; otherwise preparation refuses by
layer instead of flattening authored widths. Fonts and table typography remain
exact fields in the selected governed printing layout; map labels, halos, casing
and dash patterns remain exact fields in its resolved style documents.

## Governed attribution and context pen consumption

`Layout.attribution` is an optional closed `printing::attribution::Style` in the
same Kernel-owned Layout read by validation, native Reporter, CLI and WASM.
The standard authored LV set supplies it: governed font, ink and placement are
kept in the printing document. SVG and accessible PDF text carry the same held
policy and one mandatory licensing caption. Absence preserves the legacy
non-standard stamp path; readers never relax Layout unknown-field admission.

New context selections bind semantic `style_keys`, including the expanded model
corridor. Acquisition defaults contain geometry parameters only. The map planner
requires a held project-scoped API style-resolution table in its typed request
or captured sheets, calls `resolve_style_documents` once, and returns the sealed
style capture and exact print documents alongside the layout. Hosts forward
these documents to rendering unchanged. An absent table or unmigrated village
choice is a named refusal. The planner never writes derived pens or mutates
village strokes; independent `mv_map_context` documents own that appearance.

The existing style-key migration also completes missing keys declared by a
stored layout's selected context sources. Each requires one exact catalogue
declaration at the layout's authored ink; absent or ambiguous declarations block
the whole migration. Existing keys, inline pens, context parameters and corridor
overrides remain authored values. Its existing reviewed-plan and setup-revision
fences apply, and the replaced layout remains retained.

Legacy corridor colour, width, opacity and dash fields remain optional explicit
authored overrides for readable stored layouts. Omitted fields inherit the
resolved corridor document. They are converted mechanically into declarative
`PrintStyleOverride`, including physical `dash_mm`, never into inline generated
pens. Geometry distance and retained-source coverage remain source parameters.

### Governed renderer defaults

A render holds the sole resolver's exact reply for `(print_renderer_defaults, dataset_geojson, print, project)` beside its authored page capture. Its complete `metadata.print_renderer_defaults` object is owned by `printing::renderer_defaults::Defaults`. The object supplies the initial physical interpretation, label ink/halo/collision placement, table border/header/warning/workbook fills, furniture stroke widths, measured font family and pre-attribution licensing typography. Unknown, missing and out-of-bounds fields refuse; parallel geometry or label policy refuses. Current registered measurement/conversion supports DejaVu Sans regular/bold/italic faces. Other families refuse until their matching metrics and embedded faces are registered.

`ResolveStyles` carries the exact reply as `capture.renderer_defaults`; native preview admission carries it into `sheets.printing_renderer_defaults`. LV file exports and MV renders accept the same held Binding as `renderer_defaults`. It is companion API data: a renderer neither edits the original page capture nor looks up a mutable style head. The immutable policy reply participates in render/cache identity. Typography, colours and widths are configured through the governed document; physical conversions and placement geometry remain shared mechanics.

`map_fonts` in the same closed renderer policy owns default map title (per A0/A3), legend, scale, project and footer point sizes, bounded to 4–48 pt. Existing defaults are seeded exactly. An explicitly authored Layout retains its font sizes. Nested legacy print `text-font` face labels retain their weight/italic interpretation; the actual measured and emitted family comes from the registered governed renderer font fields. This preserves current output without claiming that legacy Open Sans labels embed Open Sans font bytes.

The same table policy declares its minimum composed font (1–48 pt), title/section/header bold choices, and the furniture policy declares legend row clearance and group width factors (0.5–5) plus group bold choice. The standard values retain current output exactly; renderers do not insert hidden typography constants.

`Layout.ink_mode` uses the same closed `style_resolution::Ink` enum as the resolver keys (`colour` or `monochrome`). Omitted colour is serialized as before. It records the authored paper identity and does not recolour a page or choose a style: every pen still comes from its exact governed key/document. The standard monochrome captures are admitted unchanged. Map composition preserves this identity from an authored base Layout.

`logo_dpi` in the same renderer policy is an integer from 72 through 1200. Its governed default is 300, matching the authored standard logo raster preparation. Physical millimetres and aspect-preserving resizing are renderer mechanics; a host does not choose its own raster resolution.

The native report host may add `ds.print-style-context/v1` to its exact-byte
`ds.print-context/v1` capture. This supplies missing selected context bindings
and the governed renderer policy through the existing Styles resolver. It never
changes the server-issued sheets JSON or its digest. Account, lane, credential
audience, project, semantic sheets digest and each final effective layout digest
are fenced. Capture and rendering share transformer-then-project override
precedence. The renderer admits exact document/revision digests and materializes
only the declared captured keys; it performs no catalogue lookup or fallback.
An existing different held document, changed configuration, or changed effective
layout refuses with a named stale/source error. Local preview receipt overlays
remain a separate API and cannot rewrite a normal delivery receipt.

The native host compares the capture against its authenticated project, lane, UID,
credential audience and the unchanged server-issued `InputReceipt.sheets_sha256`
before it binds the exact context SHA. The standalone renderer and task runner
receive no credential authority: they admit held source/document bytes only and
do not authenticate the capsule issuer. Admission precedes localization; its
ephemeral render clone consumes the capsule while immutable inputs retain it.
