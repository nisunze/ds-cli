# The district sheet — one project on one page

The per-transformer sheets are at 1:2 000–1:6 000; a district (project) sheet is at
1:30 000–1:110 000. Nothing that is right on the first is right on the second, and
99 % of the difference is template data, not code. This is the template the Gisagara
project map (2026-09-12) settled on; reuse it, do not re-invent it.

## What the sheet shows (owner's rule)

- The planned network: transformers, the LV lines **unclassified** (one pen, no cable-size
  theming), the MV routes and their **angle points**. Never poles, customers, service
  cables, spans or pole/house labels — they stay in the tables.
- Existing power infrastructure, minimal and growing: existing MV, HV, existing transformers
  (their governed pens, scaled), then substations.
- Geography: the district boundary (with its name), the sector **names without their
  lines**, the road network by class. Never cells, villages, contours or buildings at this
  scale. Admin names always carry their category — "Ndora sector", "Gisagara district",
  "Munazi cell" — except provinces (governed labels: `ds style label set --ref
  gt/<rank>_boundaries_print --field <rank> --suffix " <rank>"`).
- Places that matter, labelled by rank: district office, sector office, substation, health
  facility, university, secondary school, then primary school/training centre (on A3 the
  primary schools keep their symbol and give up their name).

## Composition

- **Portrait** for a tall district; keep an A3 counterpart. The map fills the whole page
  (`viewport.fit_around: []`); the design — MV and LV as *design* layers, so the camera
  fits both — takes the full frame with `scale_rounding` 1000 (A0) / 5000 (A3).
- **Corner snapping.** Furniture sits where the project has no infrastructure: information
  tables in the north-west corner (the neighbouring district), the legend under them, the
  title block in the south-east, the scale bar bottom-centre, the north arrow top-right.
  On A3 the authored right-hand column would eat half the width: the same corners, a
  narrower table (`widths [6,78,12,18]`, 5.5 pt).
- The district is the subject: its line over a soft band (USGS county: dashed grey 0.35 mm
  over a 1.6 mm 50 % yellow band), neighbouring districts faint (0.25 mm dashed grey), and a
  55 % white wash over everything outside the district, drawn after every context layer.
- Design colour is red (blue reads as water): LV `#C0392B` 0.28 mm dashed; planned MV
  `#B71C1C` 0.6 mm continuous; angle points hollow red circles Ø 1.5 mm. Existing MV keeps
  the house slate at 0.45 mm, HV the house orange at 0.7 mm. Roads grey by class (USGS
  100k: 0.5 / 0.35 / 0.2 mm). Transformer names `NAME · kVA` at 6.5 pt (A0) / 5.5 pt (A3).

## Pens: governed where they exist, millimetres where they do not

- A layer with a governed `_print` style keeps it; the template moves **only scale
  parameters** through `style_overrides` — `size` (css px, *not* paper-scaled: mm × 96/25.4),
  `label.size_pt`, `label.visible`, `label.priority`, and `color` where the sheet needs one
  hue. Every hidden layer needs both `visible:false` and `label:{visible:false}`.
- Legacy `styles` (mm-exact) only for the sheet's own layers: the district band, the
  neighbours, the wash, the sector names, the transformer names, the angle points.
- Two things a template cannot do today, recorded as gaps: an override cannot scale a
  governed pen's **casing** (roads print heavy at district weights — draw them with a
  `styles` pen), and `dsgrid_mv_angle_points` has no governed pen.

## Assembling the inputs headlessly

- Design: the union of the staged transformer documents (`report project export` with
  `DS_REPORT_HOST_KEEP_STAGING=1` keeps `transformer.json`/`print-context.json` per run) plus
  the MV model projected natively: `ds dsgrid run --model <.dsgrid> --operation project_plan
  --limit 5000` gives route nodes (angle points = role `intermediate`) and route edges per
  alignment; the model CRS is in `ds dsgrid inspect`. MV quantities (routes, km, angle
  points, ends) go into a `mv_quantities` layer of table rows; declare its columns in the
  config's `know_columns` sheet or the column policy empties them.
- The engine stamps every design feature's `transfo` with the request identity, so the
  governed transformer label reads one name on a merged sheet: hide it and carry `NAME · kVA`
  in the governed `names` column on a `tr_names` point layer drawn **first** (labels compete
  in draw order; the name must clear the star: radius ≥ 15 px × icon-size × 0.2646 / 2).
- Context: cut from the machine's holdings (`reference-query-cache/v2/<dataset>/<version>/
  bundle-index.sqlite`, an rtree per feature; `data project-cache status` maps layer →
  dataset/version) for a frame that covers both orientations, clip to the frame and simplify
  to ~8 m (the print budget is one million vertices), then write a `ds.print-context/v1`
  asset and its SHA-256 into the request.
- Preview at 100 dpi (A0) / 150 dpi (A3) and read detail crops at 200–300 dpi before judging.

## Reusable sheets

`ds-work/gisagara-project-map/layouts/reusable/{a0-portrait,a0-landscape,a3-portrait}-project-map.json`
with the builder (`build.py`, `geoclip.py`, `mv.py`) that authored them; publish through
`report layout update` once the validator service knows `graticule` and `title_block`.
