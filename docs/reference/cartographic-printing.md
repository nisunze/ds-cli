# Reproducible cartography through CLI and MCP

Use the deployed `ds` paired to the intended desktop lane. No source checkout or
GUI style editor is required. Begin with `ds desktop printing settings --project
<project> --output json`, then read the selected layouts with `desktop printing
get`. `ds report layout schema` returns the complete typed layout grammar.

All distances in a layout are millimetres; typography is points. Style commands
use the units advertised by `style read`, normally CSS pixels with governed
paper scaling. Keep screen and print authorship separate: create a `_print`
variant with `style print plan/create`, then edit that reference.

## Pens and labels

Run the matching `plan` first, inspect its document, then `set --yes` with the
same arguments. Add `--host desktop --project <project>` to these examples.

```sh
ds style appearance plan --ref master/lv_lines_print --size 2.3
ds style label plan --ref master/lv_poles_print --field pole_number --visible on --size 8 --paper A0 --placement auto
ds style label plan --ref master/customers_print --field house_number --visible on --size 8 --paper A0 --placement auto
ds style label plan --ref master/tr_print --field transfo --visible on --size 10 --font 'Open Sans Bold' --color '#78350F' --paper all --placement auto
ds style label plan --ref gt/rwanda_villages_print --field village --visible on --size 9 --font 'Open Sans Italic' --color '#64748B' --paper all --placement auto
ds style cartography plan --ref gt/rwanda_villages_print --visible on --opacity 0 --boundary-color '#7C8792' --boundary-width 0.7 --boundary-opacity 0.5
ds style cartography plan --ref gt/wetlands_print --fill-pattern diagonal-forward --pattern-color '#669C9B' --pattern-background '#EAF4F2' --pattern-spacing 16 --pattern-stroke 1
```

The wetland example requires an actual published source and style; it does not
create geography. Discover missing sources with `tile global catalog`, publish
eligible sources with `tile global generate`, poll `tile global status`, then
reconcile and install through `desktop data rwanda`. Use the opaque resource ID
returned by status for installation, not its human layer name.

`style cartography` also controls visibility and geometry opacity. Polygon
boundary colour, width and opacity are independent of fill. Line casing and
fill hatching retain their existing parameters. `style dimension` controls a
second field, including a black `drafting_status=draft` halo. Fonts must be in
the live `style read` label vocabulary. Omitted flags preserve authorship.

## Page furniture and tables

Every element has an editable `rect: [x,y,width,height]`. A rectangle inset from
the page edge, drawn before a slightly smaller map, provides a continuous
border and blank paper margin. Move the title-block group and its children to
give it the desired fixed width. The layout's `elements` order is draw order.

Tables use an explicit ordered binding. The following is a binding fragment,
not a complete layout:

```json
{
  "layer": "lv_poles",
  "columns": ["pole_number", "struct_type", "assembly_type", "stay"],
  "headings": ["Pole", "Structure", "Assembly", "Stay"],
  "widths": [20, 35, 45, 15],
  "sort_by": ["pole_number"],
  "panels": 3,
  "gap_mm": 4,
  "row_mm": 3.5,
  "align": "left",
  "presentation": "plain"
}
```

`widths` are relative column proportions. The renderer measures every cell and
uses the smallest width satisfying those proportions. `rect` gives the maximum
available area. `panels` is the maximum number of side-by-side panels; the
renderer uses only those needed and repeats headings. It refuses overflow
instead of silently omitting rows or shrinking text. Sorting is lexical by the
ordered `sort_by` keys, so zero-padded identifiers sort naturally.

For the styled XLSX information presentation use `layer: "workbook_info"`,
`presentation: "workbook_info"`, and columns `No.`, `Description`, `Unit`,
`Quantity`. It uses the workbook's sanitized quantities, section ordering,
markers, section fills and totals as vector cells; it remains sharp at any
print resolution. Keep its workbook row order. For compact A3 summaries choose
`layer: "lv_print_info"`, `presentation: "plain"`, and the desired ordered
summary columns. Pole and house schedules are optional page elements.

Add an optional layout-level composition instruction:

```json
{"composition":{"movable":["pole-schedule","house-schedule"],"gap_mm":5}}
```

Movable IDs must name independent tables or legends in the map viewport's
`fit_around` list. Flow-linked groups stay fixed. The renderer measures contents,
tries bounded edge and shelf placements, and ranks them by the largest free
map scale for the actual geographic aspect ratio. It preserves the authored
location when equally good. This is a deterministic bounded search, not a
claim of globally optimal packing. Fixed-scale cameras require fixed furniture;
automatic composition uses a fitted view. Impossible arrangements produce a
specific refusal so the cartographer can change columns, area or panel limits.

Legend elements accept `legend: {"layers":["lv_lines","tr"],
"titles":{"lv_lines":"New LV network","tr":"Transformer"},
"categorical":true,"row_mm":5}`. Layer order, human titles and row spacing are
authored. Category swatches use the same evaluated pens as the features.

## Corridors, scope and delivery

A `project_dsgrid_mv` context source accepts `corridor` with `distance_m`,
`color`, `width_mm`, `opacity`, and `dash_mm`. A distance of 6 means six metres
on each side. The separate `buffer_m` selects nearby models and must not be
confused with the drawn corridor. Model geometry is preserved.

Set layout `project_overview: true` and export with `--transformer
combined_transformer` to render a single sheet for all held canonical project
rooms. Without that option, combined printing retains the transformer atlas.
The overview retains outliers and refuses inputs exceeding 128 MiB or 200,000
features. The combined workflow requires the matching active project and all
requested rooms to be prepared. Its receipt explicitly states that reference
context is not yet staged by that exporter.

Save a layout and select its output using `desktop printing prepare --request
setup.json --yes`. Read settings back, export, and copy each artifact using its
returned exact `outputId` through `desktop printing artifact copy`. Archive the
layout JSON, style commands, selection and artifact receipts together as the
reusable recipe. MCP profiles expose the same typed commands and request schema.

Composition principles: [Ordnance Survey map layout guidance](https://docs.os.uk/more-than-maps/geographic-data-visualisation/guide-to-cartography/map-layout).
The measured packing approach is informed by [absolute-placement rectangle
packing](https://arxiv.org/abs/1402.0557); it is a smaller bounded implementation.
