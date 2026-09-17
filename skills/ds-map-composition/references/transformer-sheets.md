# Transformer sheets from the published default

Discover `report.layout.list|get|schema` and read the current global default for
its paper/family. Record its identity and revision. Compare the existing project
copy before making changes. Keep sourced project names, branding, engineering
styles and context; never inherit another project's wording or approvals.

## Measured placement

For the standard engineering arrangement, retain the information table, legend
and title block in the right-hand column. Their **visible right edges** align
with the title block at the page extremity unless the user specifies otherwise. Frame coordinates alone do not prove this: tables and legends shrink to
measured content. Retain the default right alignment for each binding and
stack the legend after the information table using measured content bounds.
Keep the title block at the lower-right extremity. Check the largest information
table and richest legend together against that fixed title block.

A0 schedules normally use the left extremity, filling available vertical capacity
before opening the next panel. Pole and customer schedules have independent
containers; couple them with flow only when the user requests a joined stack.
Retain a deliberate margin between schedules and between panels. Under
space pressure, different schedules may share a vertical column: one panel
ends and another begins below it with repeated headings and the saved margin.
Choose row splits and positions from measured widths, heights and free surface;
never special-case table names, source fields, row values or a fixed panel count.
Keep rows complete, ordered and readable, and preserve explicit corner anchors. Explicit corner
anchors remain fixed during automatic composition; a pole schedule anchored at
the top or bottom left must not migrate to a middle shelf. Other movable schedules
can snap to measured information/legend edges with the saved gap when useful.

The hierarchy is map space → drawn printable border → table container → table
at local (0, 0). Use the owner's frame/container controls; do not add paper offsets
or size tables from unused frame width. The thick border clips content and draws
last. Intentional flush title/right-column alignment is distinct from schedule
clearance; assess against the template's authored boundary margin.

For the right-hand stack, the corresponding supported flow is
`flow.placement: below_right`; choose a small deliberate vertical gap. Discover
these controls in the live schema before using them. A missing control is a
renderer/contract gap, not permission to invent a field or claim an approximate
frame position is anchored.

Preserve the default arrangement unless the user requests another. A corner
legend or floating table is never an improvement merely because it makes one
sample fit. User corrections take precedence over earlier drafts.

## Title and capacity

Place the transformer name at the top center of the map sheet in a reserved
band, using actual centered text alignment. The drawing-name field inside the
title block does not satisfy a top-title request. Keep all schedules below the
band. Ground the title in the canonical transformer identity. Plans display transformer
and administrative names with `.title()` casing without renaming source keys.

Use plain schedules selected from actual available engineering columns. Keep
consultant-required X/Y and use source coordinates without guessing a CRS. Show
meter numbers for as-built only where available; do not invent design values.
LV line number is not a default print column. Reserve workbook/Excel presentation
for Network Information unless the user explicitly requests otherwise.

Use predictable physical row heights, readable type and measured columns.
Inspect actual row counts and capacity, including the densest transformer.
Use A0's page height and adjacent panels; do not reserve large empty strips
between schedules or reduce the whole sheet mechanically to make A3. Compose
A3 independently from its own default. A truncated information table, ellipsis
hiding an engineering heading, or omitted schedule row fails acceptance.

## Before a full batch

Render through the saved project route and inspect whole-page previews plus
readable crops for a small, a dense and a wide transformer on every paper.
Check the visible schedule edges, panel joins, common right-column right edge,
clearance above the title block and the centered top name. A successful engine
receipt does not establish those facts. Keep those accepted previews and their
recipe revisions; only then regenerate the complete project scope and verify
publication separately. Do not call the old published PDF the new result.

Map-label omissions describe hidden text, not lost features or schedule rows.
Use the shared status presentation: label placement is informational; furniture
collisions, frame overflow and omitted schedule rows still need attention.

## Drawing-set numbering

Use the live title-block bindings for paper, current sheet, and total sheets.
Check the rendered number against the complete project drawing set, including
when an individual transformer is exported. A literal “1 of 1” in a published
default is authored text to replace, not an engine limitation. Combined PDF
assembly must retain these existing page contents and canonical order.
