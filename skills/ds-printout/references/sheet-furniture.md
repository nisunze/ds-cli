# Sheet furniture

Where tables, schedules and legends sit on a sheet, and which fields they carry.
These are standing rules, not per-sheet decisions: they are the reason the same
placement instruction stopped having to be repeated. The authority is
`ds-command-kernel/docs/printing.md`; this reference restates it for printing
work and must not drift from it.

## Sheet margin

Tables, schedules and legends hug the sheet border with one small margin — 5 mm,
the same constant as the measured content gap, so the space between content and
the page reads as the space between two content frames.

A content frame never floats in unused space. The stack starts at the margin and
grows inward, and an authored frame wider than its measured content still places
that content against the border: unused frame width is not a position.

This is the default, not a per-sheet judgement. A sheet that floats its schedules
away from the border is wrong even when nothing overlaps. Placement is a
maximisation of printed area subject to not obstructing the map — treat it as
that constraint, not as taste.

See `docs/printing.md`, "Sheet margin" and "What a sheet layout optimises".

## Headings never widen a column

`table.heading_mode` defaults to `auto_index`. When an authored heading is wider
than the data beneath it, every column takes its order letter (A, B, C...) and
the full names move to the untruncated key above the panel.

Do not shorten a heading by hand, and do not drop a column to make one fit. A
long name belongs in the key, not in a squeezed or truncated header cell.

See `docs/printing.md`, "Column headings".

## Schedule fields follow the workbook

A printed schedule carries the fields its workbook carries. Bind the schedule to
the exported table rather than re-choosing a narrower set per sheet, exactly as
the information table already works.

The customer / house-connection schedule is: Pole Number, House Number, Names,
Meter Type, Category, From Tr Distance, Nid, Service Area Length M, Village,
X, Y.

Omit the as-built-only fields when the project is not as-built — `meter_type`,
`nid`, any phone number — because a design cannot have them. Print everything
else.

A column is never dropped for width. If it does not fit, that is a placement,
panel or font decision, not a reason to lose a field.

See `docs/printing.md`, "Schedule fields follow the workbook".
