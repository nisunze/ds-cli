# Report workbook anatomy

Written against the reporter's XLSX export with the owner's 2026-09-29 rule:
voltage-drop values, sheets and columns belong in neither individual nor
combined transformer Excel workbooks. The voltage-drop report is a separate
A4 PDF output. This change is awaiting a new build and install; the currently
installed reporter can still write the old Excel content. Inspect the actual
workbook, and do not deliver one containing voltage-drop content.

## Individual transformer workbook — `<transformer>.xlsx`

Sheets appear in this order; a raw sheet is omitted when its table is empty.

| Sheet | What it is | Header |
|---|---|---|
| `InfoTable` | the bill-of-quantities summary for this transformer | after the title band: `No.`, `Description`, `Unit`, `Quantity` |
| `Dirty Categories` | only when category validation excluded or fallback-mapped a value; red tab | `Source`, `Description`, `Unit`, `Quantity` |
| `customers` | one row per customer | row 3 |
| `lv_lines` | one row per LV line segment | row 3 |
| `service_cables` | one row per service drop | row 3 |
| `poles` | the LV staking table, grouped per line | row 2, then one merged band per line |

### `InfoTable` sections, in order

Rows within a section are lettered `a, b, c …`; a section with more than one
row ends with a `Total:` row. Only non-empty sections are written.

| Section | Unit | Description column holds | Source rows |
|---|---|---|---|
| Phase Types | pce | meter type | customers |
| Customers Categories | pce | customer category | customers |
| Lv Lines | m | ABC cable type | lv_lines, new |
| Existing Lv Lines | m | ABC cable type | lv_lines, existing |
| Feeders | pce | feeder cable type | lines touching the transformer |
| Service Cables | m | service cable type | service_cables |
| Earthing | pce | `Earthing` | poles, new |
| Existing Earthing | pce | `Earthing` | poles, existing |
| Assembly | pce | assembly component code or label | poles, new |
| Existing Assembly | pce | as above | poles, existing |
| Existing Poles | pce | `Existing Poles` | poles, existing |
| Existing Pole Types | pce | structure type | poles, existing |
| Pole Types | pce | structure type (`S140`, `400daN`, …) | poles, new |
| Stay | pce | `Stay` | poles, new |
| Existing Stay | pce | `Stay` | poles, existing |
| Fly Stay | pce | `Flying Stay` | poles, new |
| Existing Fly Stay | pce | `Flying Stay` | poles, existing |
| Transfo Size | pce | `<kVA> kVA` | tr, fill-ins excluded |

`Assembly` splits a composite `assembly_type` on `;` and counts each
component (`EAT 54-10; EAS 54-10` → one of each; `EAS 54-10; EAS 54-10` →
two). An atomic label (`ABC Terminal Assembly`) counts as itself. Report
poles are `lv_poles` plus `tapping_poles`; a tapping pole shows
`struct_type = TAP`, an identity marker, not a structure to supply.

### Raw sheet columns, before blank or uniform columns are dropped

- `customers`: pole_number, house_number, names, meter_number, meter_type,
  category, from_tr_distance, nid, upi, phone_number, service_length,
  village, x, y
- `lv_lines`: line_number, cable_size, village, length
- `service_cables`: pole_number, meter_type, cable_size, service_length,
  village, length
- `poles`: pole_number, earthing, stay, flying_stay (dropped when all zero),
  struct_type, assembly_type, num_houses, dev_angle, back_span,
  from_tr_distance, material, village, x, y — `line_number` is the merged
  band title (`<line>, Cable Size: <size>`), not a column

`x` and `y` are the design coordinates as stored. `length`,
`service_length`, `back_span` and `from_tr_distance` are metres computed at
process time. Where a layer mixes existing and new rows the existing ones
are highlighted; an all-existing sheet is not.

### Separate voltage-drop report

After the new reporter build is installed, configure an existing Chrome, Edge,
or Chromium once through `ds workstation configure --component chromium
--target reporter --yes`, verify it with `ds workstation verify --component
chromium`, and select the explicit
`voltage_drop_pdf` project output, and export through `ds report project export`
or `ds desktop printing export`. The local printing engine invokes Chromium
automatically and produces `<Display Name> voltage drop.pdf` on A4, with the
extended report by default;
the brief variant is selectable. The PDF carries the `ds-lv-vd/1` result,
including criteria, outlook by year, reinforcement and drop chart. Preview
and download that published PDF on the web as its own artifact; web generation
is unavailable. Do not use either transformer workbook as a voltage-drop source.

Where a separate `<transformer>.voltage-drop.json` result is available, it is
`ds.lv-voltage-drop.result/v1` with `provenance.state`
(`as_processed` / `recomputed_at_export`), `summary` (`vd_summary`, including
`verdict`, `utility` and `stage2.outlook[]`) and projected `layers`; `report`
and `sizing` appear only when the export solved. For a check or explanation,
load `ds-lv-voltage-drop` and follow the live command contract.

## Combined workbook — `combined_transformer.xlsx`

A summary workbook: it deliberately carries no raw layer sheets. For
pole-by-pole detail open the individual workbooks.

| Sheet | Present | Shape |
|---|---|---|
| `InfoTable` | always | the same sections aggregated across the batch; title `Combined Transformer` |
| `LV Summary` | always | one row per transformer; quantity columns grouped under pivot titles; trailing `X`, `Y` (Location) and `District`, `Sector`, `Cell`, `Village` (Admin Bounds) |
| `Transformer Sizing` | when the project setting `include_tr_sizing_in_combined_report` is on (the default) | `Transformer Sizing and Protection Devices`: row 3 groups, row 4 headers each ending in their note marks (`Selected kVA [6]`), data from row 5, then `Notes`; see below |
| `Dirty Categories` | only when dirty rows exist | as above, with a `Transformer` column first |

The combined workbook has no `Voltage Drop Summary` sheet or voltage-drop
columns, including in `Transformer Sizing`.

`LV Summary` layout: row 1 pivot-title groups, row 2 a merged title band,
row 3 the description headers (rotated), row 4 `Transformer`, data from row
5. Pivot titles, in order: Phase Type(pce), Customer Category(pce), Existing
Poles(pce), Existing Pole Type(pce), Pole Type(pce), Existing Assembly
Type(pce), Assembly Type(pce), Service Cable Type(m), Existing ABC Cable
Type(m), ABC Cable Type(m), Existing Stay(pce), Stay(pce), Existing Flying
Stay(pce), Flying Stay(pce), Existing Earthing(pce), Earthing(pce),
Transformer Size(pce), Feeder Cable Type(pce), then `Feeders` / `Feeder
Count`.

`Transformer Sizing` is written as values, every one read from the project's
seeds and layers; the note a header's `[n]` points to states that value's
assumption, the project's figure and its source (REG VII table or page for an
EDCL rule set, else the rule set's own standard). Match headers without their
marks. Columns, left to right:

- No., District, Sector, Cell, Village, Site Transformer, Customers.
- Voltage Rating (`30/0.4 kV`, from the selected `transfo_sizes` row).
- Demand: Customers By Category (`Residential 41 × 250 W; …` from
  `cust_category.peak_power_w`), Connected Load kW, 1Ph / 3Ph Customers,
  Power kW (× `power_factor`), Apparent kVA (÷ `cos_phi`), Grown kVA
  (× every `growth_forecast_tr` rate).
- Sizing: Existing kVA (`None` when no installed unit), Demand kVA
  (× `simultaneity_factor_tr`; ds-network's own design demand, the figure
  its sizing selects with), Selected kVA (`plan_kva`, else `ex_tr_size`, else
  the smallest `transfo_sizes` row at or above max(demand,
  `minimum_tr_size`)).
- Protection, from that row: Primary / Secondary kV, Vector Group, I1 / I2
  (rated currents), Fuse Link A (30 or 15 kV column), LV CB A,
  Transformer–DB Cable.
- Outgoing Feeders: count, Feeder Cables, Feeder Rating A, Feeder CB A.
- Comments: the basis of the size (`plan_kva used`, `ex_tr_size used`,
  `calculated from demand; raised to minimum_tr_size=… kVA`) and any
  category counted as the default.

A cell reading `n/a` is a value the project's data cannot give; the last
note lists each with the missing input. Report it with that input, never as
zero.

## Archives

`ds report project compounded` delivers `transformers/<name>/<name>.xlsx` and
`combined/combined_transformer.xlsx`, nested by `--file-level`, and with
`--combine-per-group` each district
folder also carries its own combined set. With `--group-by` it publishes a
separate archive per leaf tag group instead, each with its own `prefix`. `ds report bundle`
produces the same layout from digest-pinned local artifacts and embeds a
`manifest.json` listing every entry with its SHA-256.
