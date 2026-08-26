# Report workbook anatomy

Written against the reporter's XLSX export as of 2026-08-26. The installed
engine is the authority: when a sheet or column named here is absent, trust
the file and note the difference.

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

## Combined workbook — `combined_transformer.xlsx`

A summary workbook: it deliberately carries no raw layer sheets. For
pole-by-pole detail open the individual workbooks.

| Sheet | Present | Shape |
|---|---|---|
| `InfoTable` | always | the same sections aggregated across the batch; title `Combined Transformer` |
| `LV Summary` | always | one row per transformer; quantity columns grouped under pivot titles; trailing `X`, `Y` (Location) and `District`, `Sector`, `Cell`, `Village` (Admin Bounds) |
| `Transformer Sizing` | when the project setting `include_tr_sizing_in_combined_report` is on (the default) | `Transformer Sizing and Protection Devices`: No., Transformer, Admin Bounds, Customers, Demand, Sizing, Protection, Site Transformer, Comments |
| `Dirty Categories` | only when dirty rows exist | as above, with a `Transformer` column first |

`LV Summary` layout: row 1 pivot-title groups, row 2 a merged title band,
row 3 the description headers (rotated), row 4 `Transformer`, data from row
5. Pivot titles, in order: Phase Type(pce), Customer Category(pce), Existing
Poles(pce), Existing Pole Type(pce), Pole Type(pce), Existing Assembly
Type(pce), Assembly Type(pce), Service Cable Type(m), Existing ABC Cable
Type(m), ABC Cable Type(m), Existing Stay(pce), Stay(pce), Existing Flying
Stay(pce), Flying Stay(pce), Existing Earthing(pce), Earthing(pce),
Transformer Size(pce), Feeder Cable Type(pce), then `Feeders` / `Feeder
Count`.

`Transformer Sizing` values are computed by the reporter and written as
numbers. `Selected kVA` follows `plan_kva` (the planner's choice) first, else
the existing installed size, else the demand-based sizing; demand, apparent
power, growth allowance, currents, fuse link and LV breaker are always
derived from the customers, even when a size was chosen.

## Archives

`ds map design batch report` delivers `transformers/<name>/<name>.xlsx`
(nested under sector or district folders when `--file-level` says so) and
`combined/combined_transformer.xlsx`; with `--combine-per-district true`
each district folder also carries its own combined set. `ds report bundle`
produces the same layout from digest-pinned local artifacts and embeds a
`manifest.json` listing every entry with its SHA-256.
