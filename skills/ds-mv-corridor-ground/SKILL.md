---
name: ds-mv-corridor-ground
description: "Seed or reseed sparse MV corridor ground through ds with reproducible scattered sampling, adaptive density and bounded surface footprints. Use for corridor DEM or survey ground, including Gisagara Model 3; not a continuous project DEM grid, triangulator implementation or implicit model publication."
metadata:
  ds-chapters: data, grid-model
---

# Seed natural MV corridor ground

Use `ds` first and `ds-terrain-sampling` for native acquisition. The owner's
2026-10-08 preference is sparse, irregular, reproducible corridor observations
with faithful terrain and readable profiles. A smooth continuous DEM provider
is not permission to seed a project-wide grid or fill the space between routes.
This skill guides sampling and authorized materialization; it does not implement
geometry or alter a triangulator.

## Establish what is being seeded

Pin the explicit project, model/history, route identities, selected alignments,
horizontal metric frame and original source identity. Read existing terrain
classes, source references, selected ground basis and coverage through `ds`.
Distinguish measured survey, derived DEM observations, nominal profile cuts and
display surfaces. Nyamagabe M1 is the owner's corridor-survey reference;
Gisagara M3's dense DEM seed is the motivating failure, not a reusable template.
Inspect actual spacing, footprint and provenance before transferring settings.
Do not transplant Nyamagabe XYZ or manufacture a survey label for Gisagara.

Discover the live terrain descriptor and sampler. Use its generated settings,
limits, preview and artifacts rather than a copied request schema. Choose one
explicit source. For this Gisagara work, Rwanda TIFF is the selected ground
provider; comparisons with another provider remain diagnostic. Preserve imported
survey XYZ and bytes. Never add random Z, blend providers, smooth elevations,
apply an unexplained offset or improve the source's claimed accuracy.

## Bound the ground to supported corridors

Define sampling extent from actual route footprints, supported runs and source
coverage in the characterized metric frame. A route bounding rectangle or a
family/project convex hull is not a corridor. Keep holes, disconnected pieces,
no-data and unsupported intervals open. Dense coverage elsewhere does not support
an empty patch here. At route ends, enforce bounded station eligibility rather
than clamping outside observations into endpoint stacks.

Alignment identity supplies a maximum ownership/dependency boundary, not proof
of spatial support. Actual footprint overlap and local two-dimensional sampling
density determine which observations can support each bounded piece. Nearby or
intersecting routes may reference one physical observation through plural
membership; do not copy it or force it onto a single nearest alignment. Connected
T-offs may share declared pieces. Independent principal families retain separate
interpolation domains even when they share observations. Overlap is evidence for
bounded sharing, never permission to merge families or bridge their empty space.

A dense DEM input must be sampled down to the authorized corridor footprint;
it does not justify importing every raster cell. A sparse survey does not become
a continuous surface just because a global Delaunay triangulation can connect it.
Require locally supported triangles/pieces, density-aware edge admission and
explicit gaps. No universal distance cutoff or one-surface-per-label shortcut
substitutes for these checks. A degenerate or unsupported patch remains missing.

## Random, sampled and beautiful

Record a reproducible seed. Use bounded longitudinal jitter for candidate
stations and separately evaluated lateral randomness for scattered observations.
Choose widths from the engineering task and source support, not the map extent.
Prefer an irregular corridor population over a uniform rectangular lattice;
avoid decorative clusters and duplicated centerline/side observations.

Randomness changes query positions only. Every height comes from the selected
unchanged surface at that sample's actual XY. Preserve exact route endpoints,
PIs, engineering bounds, coverage transitions and significant sampled crests or
troughs. Retain enough local density to meet both the native discrete height-error
criterion and the strictest applicable station/span gap. Flat terrain can thin;
sharp terrain needs tighter spacing. Supply real engineering intervals and
weight spans when available; route length is a named fallback, not weight-span
certification. Source resolution, uncertainty, baseline spacing and thinning
error remain separate facts.

Build centerline ground at exact route XY. Never sort alternating off-route
heights by station and join them as ground: a transverse slope creates sawteeth.
Optional side cuts stay at their nominal signed offsets; separate random side
observations cannot move those cuts. Discover the native sign convention rather
than borrowing one from another profile API. Read
[references/quality-and-contracts.md](references/quality-and-contracts.md) for
visual acceptance, negative controls and the current native contracts.

Beautiful means an honest, legible corridor: irregular point distribution,
terrain-following density, coherent nominal cuts and visible gaps. It never means
removing real ridges, flattening a valley or inventing coverage for appearance.

## Verify, then cross the model boundary explicitly

Inspect native artifacts and receipts for source/input digests, settings, seed,
actual XY/Z, widths, density, retention reasons, discrete error and gaps. Inspect
plan plus centerline/side profiles at relevant scales. Check separated corridors,
overlap/junctions, route ends, a steep cross-slope and a sharp crest. Dry-run
validates the plan only; it cannot establish provider coverage.

Acquisition does not edit a model. For an authorized import/reseed, follow
`ds-grid-project-model` and discover the live terrain materialization and
alignment ground-basis commands. Preserve original observations and their source
classes. Bind derived cuts with explicit stations and supported-run boundaries;
profile, pole ground, attachments, spotting and clearance must consume the same
resolved ground. Preview at the exact head, apply through the native transaction,
and read back provenance, basis, counts and gaps. Save and project publication
are separate authorized effects. A request for this skill alone authorizes none
of those model changes.

The October 8 transactional Profile contract describes required bounded domains,
not proof that an installed binary implements them. If the live surface cannot
preserve footprint/membership/gap semantics, stop that materialization. During
coding, use the owning code/decision/seeding backlogs; during ordinary product
use follow `ds`'s governed sighting workflow. Do not fill the gap with a script,
hand-edited package or relaxed dependency guard.

Stops at: verified native corridor-sampling artifacts, or the explicitly
authorized native model import and readback. The operator resolves unavailable
authority; native PLS-CADD remains the engineering acceptance boundary when
required by the delivery.
