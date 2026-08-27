---
name: ds-report-consumption
description: Obtain DS report workbooks through `ds` and read sheets, units, existing/new rows, and dirty categories.
---

# Consume a delivered report workbook

The stack ends at the workbook. `ds` produces transformer and combined report
artifacts through the reporter engine, and every quantity in them is computed
from the saved design geometry. Reading a delivered workbook, joining it with
an engineer's own spreadsheet and reasoning about the result is your job, with
whatever spreadsheet reader your environment provides. That is not a `ds` gap
and needs no feedback report. The `ds` skill's rule against substituting a
parser applies to DS surfaces — `.dsgrid` packages, application storage, the
desktop bridge — never to a document the reporter has already delivered.

Never reconstruct a quantity from raw layers, the map or your own geometry
when the workbook carries it. If the workbook does not carry it, say so; do
not estimate.

## Obtain the workbook through `ds`

Discover the live contracts first (`ds capabilities report --output json`,
`ds capabilities map.design.report --output json`); the shapes below are the
ones this skill was written against.

- `ds map design report --transformer <name> --yes --output json` — runs the
  local reporter lane for one saved transformer. The receipt is evidence, not
  a path: filename, size and SHA-256 per artifact. A report the freshness
  gate holds back says so instead of regenerating. The application keeps the
  artifact and syncs it to the project; take the file from the project's
  report artifacts and check its SHA-256 against the receipt before reading.
- `ds map design batch report --transformer A --transformer B … --file-level
  transformer|sector|district|root --yes --output json` — 2 to 200
  transformers; one archive holding `transformers/<name>/<name>.xlsx` (nested
  by the chosen level) and `combined/combined_transformer.xlsx`. Read the
  archive URL, individual coverage, missing and error counts from the
  receipt; download the archive with an ordinary HTTP client.
- `ds report export --task transformer|combined … --out-dir <dir> --output
  json` — the engine directly, over local typed inputs, writing files into
  `--out-dir`. Use it when you already hold the transformer documents; the
  request schema is `ds report tasks --task <name>`.
- `ds report bundle --request <file>` — repackages digest-pinned artifacts
  into one ZIP with `manifest.json`; it never creates quantities.

A blocked export returns typed blockers in the refusal detail. Read them
back to the user; do not retry unchanged.

## Read it correctly

Open the file with any spreadsheet reader you have. Every cell is a plain
value — the reporter writes no formulas, so nothing needs recalculating.

1. **Find headers by name, not position.** A title band precedes the header
   (one row on `poles`, two on the data sheets; `LV Summary` uses four header
   rows). Headers are Title Case of the column ids (`Pole Number`, `From Tr
   Distance`); material codes keep their spelling.
2. **Expect columns to be absent.** Entirely blank columns are dropped, and a
   categorical column whose every value is the same (`material` on an
   all-wooden network) is collapsed away. An absent column is not missing
   data; read it as "uniform or empty" and say which.
3. **Keep `Existing …` apart from new.** Most InfoTable sections have an
   existing twin (`Existing Pole Types`, `Existing Stay`, …). Existing rows
   are as-built and never supplied; only the plain sections are demand.
   Fill-in transformers are excluded from `Transfo Size` on purpose.
4. **Treat `Dirty Categories` as unresolved quantity.** The sheet exists
   (red tab) only when category validation excluded or fallback-mapped a
   value. Surface every row; never fold one silently into a total.
5. **Units live in the section, not the row.** `pce` counts and `m` lengths.
   InfoTable quantities are rounded to two decimals, so re-summing a raw
   sheet can differ by a few centimetres — rounding, not a discrepancy.
6. **`poles` is grouped per feeder line.** Each merged band reads
   `<line>, Cable Size: <size>`; fill-in feeders are styled differently and
   stay in the deliverable.

Sheet-by-sheet columns and section order:
[references/workbook-anatomy.md](references/workbook-anatomy.md).

## Return bounded evidence

Name the file, its SHA-256, the sheet and the rows you used. Quote quantities
with their unit and section. Where you computed a join or a sum, show the
inputs so an engineer can redo it in the spreadsheet. Do not paste whole
sheets back.

For a pole-by-pole staking comparison load `ds-boq-staking-table`; for a
project BOQ against the combined workbook load `ds-boq-combined-report`.
