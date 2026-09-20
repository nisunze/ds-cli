# Sheet furniture

Use the live Rust layout contract and the published paper/family baseline.
[Transformer placement](../../ds-map-composition/references/transformer-sheets.md)
owns the visual reasoning; [outlier adjustments](../../ds-map-composition/references/outlier-transformers.md)
explains isolated exceptions. Do not substitute a historical workbook recipe.

## Boundary and placement

The drawn thick border is the printable domain and clips all content. Each
schedule has its own container and local table origin; unused maximum frame
width is not measured table width. Retain physical gaps between panels and
schedules. Use available height before continuation. Flow deliberately couples
containers; independent schedules remain independently adjustable.

Explicit corner anchors constrain automatic placement and skyline capacity
profiles. Other movable furniture may snap to measured information/legend edges
with the saved gap. Keep the title block grounded lower right and information
above it with a common visible right edge where that is the accepted arrangement.
Flush alignment at the border and inset schedule clearance are different choices.

## Fields and headings

Use selected columns from actual transformer engineering data for plain schedules,
with Design/AsBuilt phase. Preserve required X/Y using source values and their
known CRS. Meter number matters for as-built where available; unavailable design
values stay unavailable. Do not add LV line number by default. Network Information
can retain workbook presentation; schedules use it only when explicitly requested.

Choose the live heading treatment rather than abbreviating engineering names to
force a fit. Indexed headings can carry a complete key above each panel. Frame,
panel and type adjustments must preserve required fields and complete rows.

## Values and legend

A column whose values recur in a few categories (Category: Residential /
Commercial; Meter type: Single Phase / Three Phase) can print number indexes
with one legend line per column under every panel, in combination with the
heading key above it: `Category: 1 Commercial · 2 Residential`. Indexes are
1-based in case-insensitive value order; empty cells stay empty; the legend
names the full heading, never its alias. The row plan is given the height
minus the legend, so rows never overprint it. The kernel decides; every
renderer paints the same plan.

`table.value_key` on the table binding (and in a per-transformer table
override) carries `mode`, `max_categories` (2–26, default 3) and `columns`:

- absent → the table's own default: `auto` for the customers (house
  connection) schedule, plain `customers` layer or workbook `customers` sheet;
  `off` for poles and every other table;
- `auto` → every bound column with at most `max_categories` recurring values
  whose widest value is wider than its index compacts; a legend that leaves the
  frame no room for its rows is dropped and the values print literally;
- `columns` → exactly the listed bound columns compact; a column with more
  categories than allowed refuses naming the column and its count — raise
  `max_categories` or drop the column;
- `off` → every value prints literally.

Edit with `report.layout.edit` op `edit` carrying the complete table element,
e.g. `"table":{...,"value_key":{"mode":"columns","max_categories":3,"columns":["category","meter_type"]}}`;
`{"mode":"off"}` silences the customers default. Inspect one real render: the
`1`/`2` cells, the legend under the last row and the untouched poles table.

## Diagnostics

Read the shared status output. Label-placement notes count map text hidden to
avoid overlap or clipping; they do not imply missing source records. Furniture
findings and schedule row omissions remain problems to resolve. Table schematics
are compact editing aids, not measured export geometry: inspect a real render.
