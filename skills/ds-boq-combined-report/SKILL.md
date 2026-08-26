---
name: ds-boq-combined-report
description: Reconcile the combined workbook (InfoTable, LV Summary, Transformer Sizing) with a project BOQ — align items, explain deltas.
---

# The combined report against a project BOQ

An EPC bill of quantities for a distribution lot is, in its LV part, the
combined workbook re-expressed in the contract's own item numbers: cable
lengths per ABC size, poles per type, assemblies per code, stays, earthing,
service cables per size, meters per phase, transformers per kVA. The
workbook is the design's answer; the engineer's BOQ is the contract's. Your
job is to align them and explain every delta, not to produce a third figure.

`ds-report-consumption` governs how the workbook is obtained and read. Use
the combined workbook for the batch (`ds map design batch report`, 2 to 200
transformers, or `ds report export --task combined`); open the individual
workbooks only when a delta needs pole-level detail.

## Which sheet answers which question

- **Lot totals** — `InfoTable`: one figure per description, sections in the
  fixed order from Phase Types to Transfo Size. Read only the plain sections
  as demand; every `Existing …` section is as-built and unpriced.
- **Per-transformer breakdown** — `LV Summary`: one row per transformer,
  quantity columns grouped under pivot titles, then `X`, `Y`, `District`,
  `Sector`, `Cell`, `Village`. Use it to find which transformer carries a
  delta and to split a lot BOQ by district or sector.
- **Transformer lines** — `Transformer Sizing`: `Selected kVA` per
  transformer with the demand it was sized from, protection (fuse link, LV
  breaker, outgoing CBs) and the admin bounds. A BOQ transformer line that
  disagrees with `Selected kVA` is a planning decision (`plan_kva`) or an
  existing unit, not an arithmetic error; say which. Fill-in transformers
  reuse an existing unit and are absent from `Transfo Size` on purpose.
- **Unresolved rows** — `Dirty Categories`: values excluded or
  fallback-mapped by category validation. Every dirty row is quantity the
  BOQ may be carrying under another name; list them all.

## Align, then compare

1. Build the alignment table first: BOQ item → workbook section and
   description, with the unit on each side. Typical joins: ABC cable
   `3x70+54.6` ↔ a `Lv Lines` description; service cable `2x16` or `4x16` ↔
   `Service Cables`; pole class ↔ `Pole Types`; assembly codes (`EAS 54-10`)
   ↔ `Assembly`; single- and three-phase meters ↔ `Phase Types`; kVA ↔
   `Transfo Size`. Descriptions rarely match verbatim (`AAAC 3x50` against
   `50 mm²`, km against m, `pcs` against `pce`). Propose the mapping, show
   it, and let the engineer confirm it before any total depends on it.
2. Convert units explicitly and keep the factor in the table. The workbook
   rounds InfoTable quantities to two decimals; a difference below that is
   rounding, not a finding.
3. Sum deterministically — a small script or the spreadsheet itself — and
   set three columns side by side: BOQ, workbook, delta. Add a fourth for the
   cause once you know it.
4. Attribute each delta with the finer sheet: `LV Summary` to the
   transformer, then that transformer's own workbook (`poles`, `lv_lines`,
   `service_cables`) to the rows. Common causes: existing assets counted as
   new on one side, tapping poles billed as poles, fill-in transformers
   billed, a feeder redesigned since the BOQ was priced, a dirty category, a
   BOQ allowance (spares, wastage, contingency) the design never carries.
5. Leave BOQ lines the workbook cannot answer — civil works, transport, MV
   items, meter accessories, labour — unmatched and say so. Do not estimate
   them; for MV structures use `ds-boq-staking-table`.

## Return

The confirmed alignment table, the BOQ/workbook/delta table with causes, the
unmatched BOQ lines, the dirty rows, and the file names with SHA-256 for both
sides. Keep it bounded: totals and the rows that differ, not every row.
