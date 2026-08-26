# PLS-CADD deviation-route and terrain round trip

This procedure is for one bounded native backup plus explicit route and point
evidence. Command flags below are routing hints, not a copied contract: read
each live descriptor before invoking it.

## 1. Establish the installed surface and evidence set

Run `ds doctor --output json`, then discover the `dsgrid`,
`dsgrid-exchange`, and relevant report commands. Record, without modifying:

- every input path, byte length and SHA-256;
- the selected `.don` project when the backup contains more than one;
- the authoritative horizontal CRS and vertical datum;
- each route layer's intended alignment and authority status;
- each point's source, measured coordinates/elevation, requested code, and
  whether it is surveyed, derived, or merely proposed;
- the PLS-CADD version that must restore and open the result.

Do not infer authority from filename recency. When an approved model and a
derived route disagree, ask which governs before applying geometry.

## 2. Import the native backup

Use the live descriptors for `dsgrid-exchange.inspect`,
`dsgrid-exchange.plan`, `dsgrid-exchange.convert`, and `dsgrid.validate`.

Inspect the `.bak` directly. Plan a `dsgrid` target with the explicit CRS and,
when known, expected longitude/latitude/radius audit. Read every blocker,
warning and declared loss. Convert into a new empty output directory, retain
the exchange report, then validate the emitted `.dsgrid`.

Stop if project selection is ambiguous, source digests change, native
references are unresolved, the location audit disagrees, or the canonical
model is invalid. Never fall back to basename matching or a hand-unpacked
folder merely to bypass a blocker.

## 3. Reconcile the deviation route

Project the canonical model into the smallest useful GIS review artifact and
compare the exact incoming line with the existing ordered alignment. Preserve
branches, taps, route direction and shared junctions. A crossing does not
prove connectivity; a near-parallel line may be survey drift or a distinct
corridor.

Use `ds dsgrid describe --kind commands` and then read only the chosen command
descriptor. Typical owner operations are:

- `replace_alignment_route_restationed` when an existing alignment's ordered
  vertices change but structures must keep absolute XY while station/offset
  are re-derived;
- `create_alignment_route` for a genuinely new alignment;
- `move_route_node` only for one already-identified canonical PI whose local
  move does not require a complete-route replacement.

The engine command envelope must carry a stable command id, the current
authored revision, the installed command schema version, and the typed command
payload, and the payload names its command kind as a tagged field. Run
`ds dsgrid apply --dry-run` first. Apply only when the preview introduces no
validation errors, and write a new `.dsgrid` for every accepted revision.
Re-inspect and validate each output before using it as the next parent.

Never confuse the manifest's monotonic package revision with the engine's
content-derived authored revision. The apply receipt reports both. No
read-only command prints the authored revision of a freshly converted package:
take it from the previous apply receipt, or let one dry run refuse with a
revision conflict and read the actual revision from that refusal. The
descriptor names row and enum types without their fields; when a dry run
refuses the envelope, read its detail and rebuild rather than guessing twice —
and report the missing shape through `ds feedback`.

## 4. Add terrain and elevation evidence

There are three distinct operations:

1. `author_terrain_source` records provider/dataset, horizontal CRS, vertical
   datum, acquisition time and nominal resolution.
2. `author_terrain_points` adds a surveyed or acquired batch with explicit
   XYZ and source provenance as one journaled revision.
3. `insert_terrain_point_at_station` places a new point on the canonical
   alignment and interpolates its elevation from existing effective ground.
   It intentionally has no source id because it is derived, not surveyed.

Prefer a single batch command for a point set; do not mint thousands of
single-point revisions. Preserve measured elevations exactly. Use station
interpolation only when that is the operator's intent and ground coverage is
verified. A `terrain_acquisition_required` refusal is a hard evidence gate,
not permission to linearly invent a value.

Validate DEM or other external terrain against surveyed ties. Refuse a
constant-offset or smoothing repair when residuals vary materially or change
sign; that shape indicates datum/surface disagreement rather than one offset.

Test whether a "survey" feed is really a survey before choosing its source
kind. Elevations that are all multiples of 1/256 (`1655.671875`,
`1672.023438`) are float32 raster samples, and evenly spaced `Gp` rows along a
digitised line are generated, not walked. Compare the feed against the
project's own surveyed terrain at every coincident point (or a bounded-edge TIN
over the existing corridor) before authoring: on one MV deviation set the feed
sat a median +12.4 m above the project ground with a p10–p90 spread of
+8.3 to +17.8 m. Author such rows under a `dem_raw` source with an `unknown`
vertical datum, keep their heights exact, say so in each description, and put
the datum ruling to the operator; never shift them by the median.

The engine's own derived observations (`insert_terrain_point_at_station`)
carry no source id; rows you derive from a DEM feed (gap fills between its
own samples) belong to that feed's source, not to a surveyed one.

Provenance does not survive the native round trip: a re-imported backup shows
one `pls_cadd_import` terrain source however many you authored. Descriptions
do survive, so the provenance word has to be in the description too.

For points intended to guide manual PLS-CADD PI movement:

- retain the point layer as terrain/survey evidence with its real project
  feature-code token;
- ensure each intended native PI also exists as an ordered route vertex;
- do not call a terrain point an angle point solely because its text says
  `PI`;
- do not derive PLS line angle from a report column that is zero away from
  structure-coincident PIs.

## 5. Export the DS revision back to PLS-CADD

Re-run `dsgrid.validate`, then inspect and plan a `pls-bak` conversion from the
accepted `.dsgrid`. Review the plan's source-version intent, route/terrain
coverage, preserved opaque assets, exchange bindings, losses and output names.
Convert into a new empty directory; never overwrite the input backup or an
earlier candidate.

The export must retain exact structure/cable/criteria/parts leaves and their
adopted bytes. Alignment, PI geometry, placements and terrain are actionable
model state; approved library resources are not regenerated from a reduced
representation.

Independently read back the emitted backup through `dsgrid-exchange` and
compare canonical counts, route vertices, terrain content, structure plan
positions and digests. This is deterministic exchange evidence, not native
acceptance.

The owner's ruled flow is backup → canonical workspace → deviations and
interpolated points → a PLS-CADD **workspace folder** the operator edits in
place. Export with the folder container, not a new `.bak`: the operator opens
the emitted `.don` directly, and the return trip re-imports the same folder.
Prove the folder with the reference-closure command (every reference bound
inside the folder, none unresolved) and a readback before handing it over;
reserve a `.bak` for archival or for a machine that cannot see the folder.

A review overlay for the operator — a corridor buffer around each deviation,
a centreline the operator can attach in PLS-CADD — is not a `ds` capability
today and is not model state: build it beside the handoff as SHP and DXF in
the project CRS, hand the operator the attach step, and keep the buffer width
and source digests in the delivery note.

## 6. Native operator loop

Restore the candidate into a fresh directory in the declared PLS-CADD version
and reopen it. Require zero project-moved, missing-file, problem-reading, and
same-name file/directory errors. The operator then:

1. moves the intended native alignment PIs;
2. readjusts structure positions and orientation as engineering judgment
   requires;
3. runs the relevant native checks/reports;
4. saves and creates a new native `.bak`.

Do not edit underneath the running PLS-CADD instance; it caches resources and
can report against stale bytes.

## 7. Re-import, analyze and deliver

Treat the returned native backup as a new input with a new digest. Import it
through the same inspect/plan/convert sequence. Compare it against the exact DS
candidate given to the operator and report at least:

- added, removed and moved PIs by alignment and station/order;
- route length/direction and junction changes;
- terrain points and elevations added, removed or changed, including codes and
  provenance;
- structures whose XY, station/offset, elevation, orientation or type changed;
- reference-closure and exact-leaf library preservation;
- analysis/check status, warnings and unjudged scope;
- generated native and DS report artifacts with digests.

Package reports from the authoritative returned revision. A report that opens
or a package that exists does not imply engineering approval; state that gate
separately.
