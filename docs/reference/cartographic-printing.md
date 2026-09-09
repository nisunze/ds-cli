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
ds style cartography plan --ref gt/rwanda_villages_print --visible on --opacity 0 --boundary-color '#7C8792' --boundary-width 0.7 --boundary-opacity 0.5 --boundary-line-type dashed
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
  "max_widths_mm": [20, 35, 45, 15],
  "sort_by": ["pole_number"],
  "panels": 3,
  "gap_mm": 4,
  "row_mm": 3.5,
  "align": "left",
  "presentation": "plain"
}
```

With `max_widths_mm`, each column independently auto-fits the measured contents,
up to its ordered maximum. Outliers are truncated with an ellipsis; whitespace
and line breaks become single spaces. Cells remain one line at the authored
font size. Omit the caps to retain proportional `widths` sizing. `rect` gives the maximum
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

## Source selection, print focus and hierarchy

Add synchronized surveyed features with a context source such as
`{"id":"survey_existing_poles","label":"Surveyed existing poles","source":{"kind":"survey","form":"lv_poles_as_built"}}`.
Create its governed print style with `style print create` and bind it through
`style_refs`. Explicit selection is independent of map visibility. Empty or
incomplete caches refuse the print and name the missing source.

A held local layer uses `{"kind":"local_layer","layer_id":"<id>"}`. Create or
import it through `map draw`/the live local-data command, discover its ID, and
bind a template-local `styles` pen or governed `style_refs` entry. Complete
payloads are required: display previews are never mistaken for all features.
Survey/local captures are bounded to 50,000 features and 32 MiB each.

Set `layer_order` to logical layer IDs, back to front. For example:
`["contours_intermediate","contours_index","village_boundaries","buildings_context","customers","roads","service_cables","lv_lines","dsgrid_mv_lines","spans","survey_existing_poles","lv_poles","dsgrid_mv_structures","tr"]`.
Unlisted layers follow in original input order; include every selected layer
when a strict hierarchy is needed. Label priorities are separately authored:
`style label set --number symbol-sort-key=-1000` reserves transformer labels
first. `--overlap on` tries clear positions then allows an overlap if necessary.

A template `focus` such as `{"buffer_m":35,"outside_color":"#E1E5E8","outside_opacity":0.28,"border_color":"#9AA5AE","border_width_mm":0.15}` derives
an expanded area from the current transformer's bounding geometry and washes
the outside. Both buffer and fit exist only for that export. Saved transformer
boundaries and other transformers are never changed.

Span-only labels use `style cartography set --ref master/spans_print --opacity 0`
and `style label set --ref master/spans_print --field length --format round
--suffix ' m' --paper A0 --alignment line-center --overlap on --number symbol-sort-key=-500`.
The separate LV conductor layer keeps its own pens. Labels are aligned to the
span and kept upright. To label transformer name and rating, use `--field
transfo --append-field tr_size --separator ' · ' --suffix ' kVA'`.
`--number property=value` exposes backend-bounded numeric label controls;
`--halo-color` controls text halo colour. These options are also typed MCP inputs.

## Global defaults, project and personal overrides

The existing layout save contract governs global/project publication. Save the
reviewed starting template globally; a project layout with the same ID provides
its override. Save only the intended scope and use its returned revision for
optimistic concurrency. Selecting a project layout does not modify other
projects' selections. Local one-off rendering can read a request file through
`report layout render` without publishing a template.

`style_overrides` provides per-template pens over shared governed symbols:
`{"lv_lines":{"size":2.3,"palette":{"3 x 35 + 54.6mm² ABC":"#A88D00"}}}`. Only selected category
colours change; other categories and icons are inherited. Fields are `color`,
`size`, `opacity`, and `palette` (category value to colour). A palette requires
a primary categorical match/get colour style, or an explicit `field`. These overrides apply to both
map and legend. A global default, project override and personal render can
therefore keep different pens without repeatedly editing shared styles.

Per-template label overrides are independent of shared Style Center settings:
`{"tr":{"label":{"size_pt":8,"allow_overlap":false,"priority":-1000}},"spans":{"label":{"visible":false}}}`.
`label` also accepts `color`, `halo_color`, and `halo_mm`. Point sizes and halo
millimetres are physical and stay fixed across the inherited paper factors.
Use this for a generalized project overview while detailed network sheets keep
A0 span lengths and their engineering labels. A3 omits span labels. Geometry remains in the input
and therefore still contributes to the full project extent.

Polygon outlines can be styled independently: `style cartography set --ref gt/rwanda_villages_print --boundary-line-type dashed --host desktop --project PROJECT --yes`. Use `solid` to clear boundary dashes; fill opacity remains independent.

Automatic fitting accepts `scale_rounding: 100` on a layout to round the actual scale denominator upward to the next hundred without cropping. Explicit camera scales remain exact. Scale bars measure their frame, choose a 1/2/5 ground distance, and draw alternating segments with zero, midpoint and unit-bearing endpoint labels; remove a separate `scale-background` rectangle for a clean unboxed bar.

A template override may supply `field`, `palette` and `size_by_value` to classify a previously flat style. For example `{"field":"type","size":2.8,"palette":{"District Road":"#D6D5D1"},"size_by_value":{"National Road":4.4,"District Road":3.4}}`. Inspect the exact values through `desktop printing seed-context`: its bounded `propertySample` reports up to 16 values per field from the first 1000 features, not a complete domain. Shared pens remain inherited for unspecified categories. Polygon labels search within the visible polygon, keeping their full text box outside holes and furniture.

For independent draft body transparency, keep the authored halo and add an opacity dimension: `style dimension plan --ref master/lv_poles_print --field drafting_status --channel opacity --value draft=0.5 --other 1 --keep-other-channels`. Review, then use `set --yes`. The same flag works for screen styles. Sprite body opacity is baked independently of the halo, so zero hides the body while retaining the ring. Halo colour can carry alpha when the ring itself should fade. Print and live raster symbols remove the interior from the halo before painting the body, avoiding a black silhouette beneath a translucent sprite.
