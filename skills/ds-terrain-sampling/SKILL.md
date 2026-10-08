---
name: ds-terrain-sampling
description: "Create representative elevation samples along a route through native ds terrain sampling, with adaptive density, reproducible randomness and optional side profiles. Use for terrain curves and survey or Rwanda surface sampling, not fixed-distance geometry points or model publication."
metadata:
  ds-chapters: data
---

# Representative terrain samples

Use deployed `ds` and its native request contract. Discover the installed
surface before preparing input:

```
ds capabilities --search terrain --output json
ds capabilities data.terrain.describe --output json
ds data terrain describe --output json
```

Read the generated schema, defaults, source semantics and limits. Then read
`data.terrain.sample` and prepare a request matching that live schema. Request
paths resolve relative to the request file. CLI and the `ds_data` MCP router
reach the same Rust sampler; the `datasets` typed profile exposes both commands.
Missing installed capability follows `ds` discovery. During coding, use the
owning backlogs; outside coding, use the governed sighting workflow. Never
replace a missing capability with a skill-local interpolation script.

For sparse MV corridor seeding or reseeding, also use `ds-mv-corridor-ground`.
Its footprint and domain rules prevent a dense DEM input from becoming a
project-wide ground grid or a hull bridging unsampled routes. Local footprint
overlap and sampling density permit bounded observation sharing; independent
alignment families still retain independent interpolation domains. Alignment
identity alone neither proves coverage nor requires duplicate physical points.

Plan disconnected acquisition/query footprints before surface construction:
actual center and enabled/planned side cuts, signed corridor offsets and bounded
lateral observation randomness, with only necessary local source support.
Possible side profiles need an explicit finite allowance. Membership uses actual
XY and bounded station. A whole-source mesh clipped afterward is not bounded
work. Opposite-corner alignments must not query or fill their enclosing country
area; count source blocks/bytes, queries and triangle work, not only output rows.
Bounded shared metadata and block/interpolation padding are separate costs.

Choose one source explicitly. A provider comparison diagnoses differences;
it does not authorize mixing providers, correcting heights or applying a datum
offset. Survey input must declare its actual coordinate frame and keep its
original XYZ and bytes. An imported label alone does not prove field
measurement. Generated profiles and scattered samples remain derived, even
when their source is a measured survey.

Choose settings from the question and source, rather than copying a past
example distance. Keep enough baseline queries to detect the smallest terrain
feature that matters; the retained curve must satisfy the requested height
error against those queries. A narrow crest may need nearby retained points.
A flat section can use wider spacing only while its density constraints still
hold. Supply actual engineering intervals and weight spans when available;
route length is an explicitly reported fallback, never a computed weight span.
Without that context, report the conservative global density and its limits.
Sampling spacing, source resolution, source uncertainty and reduction error
are different quantities; denser sampling cannot improve a raster's accuracy.

Side profiles default to off. Enable only the signed widths needed for the
question. A width defines a constant nominal cut relative to the route;
longitudinal randomness and adaptive thinning choose stations on that cut.
Disabling side profiles does not disable the separate corridor observations.
Optional random side observations are separately evaluated at their actual
XY. Never chain alternating corridor observations as a centerline or side
profile: transverse slopes create artificial sawteeth. Reproducible seeds
change query positions, never source measurements or elevations.

Inspect the output files and receipt: input/source digests, effective settings,
retention reasons, gaps, discrete height error and engineering density evidence.
Compare a denser baseline when the feature scale remains uncertain. Missing
readings remain gaps. A discrete check does not prove continuous unsampled
terrain, and authored breaklines need a surface that supports them.
Dry-run validates the inputs and query plan; it does not acquire or evaluate
the provider and cannot report source coverage.

For fixed-distance points without terrain, use `ds-vector-tools`. For changing
terrain in a design, hand the sampled artifacts and provenance to
`ds-grid-project-model` and follow that live model's typed contract. Local
sampling does not authorize a model edit or publication.

Stops at: verified local sampled artifacts, or the native model workflow for
an explicitly authorized import. Preserve the actual provider and input
identities at that boundary.
